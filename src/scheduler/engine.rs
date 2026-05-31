use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::{history_repo, task_repo, DbPool};
use crate::models::{RunStatus, Task, TaskStatus, TriggerType};
use crate::scheduler::executor;

#[derive(Debug)]
pub enum SchedulerCommand {
    Reload,
    Pause,
    Resume,
    TriggerNow(Uuid, oneshot::Sender<Result<()>>),
    Shutdown,
}

struct ScheduleEntry {
    next_run: DateTime<Utc>,
    task_id: Uuid,
}

impl PartialEq for ScheduleEntry {
    fn eq(&self, other: &Self) -> bool {
        self.next_run == other.next_run && self.task_id == other.task_id
    }
}

impl Eq for ScheduleEntry {}

impl PartialOrd for ScheduleEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduleEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.next_run
            .cmp(&other.next_run)
            .then(self.task_id.cmp(&other.task_id))
    }
}

pub struct SchedulerEngine {
    pool: DbPool,
    cmd_tx: mpsc::Sender<SchedulerCommand>,
    cmd_rx: mpsc::Receiver<SchedulerCommand>,
}

impl SchedulerEngine {
    pub fn new(pool: DbPool) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        Self {
            pool,
            cmd_tx,
            cmd_rx,
        }
    }

    pub fn command_sender(&self) -> mpsc::Sender<SchedulerCommand> {
        self.cmd_tx.clone()
    }

    pub async fn run(mut self) {
        info!("Scheduler engine starting");
        let mut heap: BinaryHeap<Reverse<ScheduleEntry>> = BinaryHeap::new();
        let mut paused = false;

        self.load_tasks(&mut heap);

        loop {
            let next_entry = heap.peek().map(|e| e.0.next_run);

            tokio::select! {
                biased;
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        SchedulerCommand::Reload => {
                            info!("Reloading all tasks");
                            heap.clear();
                            self.load_tasks(&mut heap);
                        }
                        SchedulerCommand::Pause => {
                            paused = true;
                            info!("Scheduler paused");
                        }
                        SchedulerCommand::Resume => {
                            paused = false;
                            info!("Scheduler resumed");
                            heap.clear();
                            self.load_tasks(&mut heap);
                        }
                        SchedulerCommand::TriggerNow(task_id, reply) => {
                            let result = self.trigger_task(task_id).await;
                            let _ = reply.send(result);
                        }
                        SchedulerCommand::Shutdown => {
                            info!("Scheduler shutting down");
                            break;
                        }
                    }
                }
                _ = async {
                    if let Some(next) = next_entry {
                        let duration = (next - Utc::now()).to_std().unwrap_or(std::time::Duration::ZERO);
                        tokio::time::sleep(duration).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                }, if !paused && next_entry.is_some() => {
                    if let Some(Reverse(entry)) = heap.pop() {
                        if entry.next_run <= Utc::now() {
                            self.execute_and_reschedule(&mut heap, entry.task_id).await;
                        } else {
                            heap.push(Reverse(entry));
                        }
                    }
                }
            }
        }
    }

    fn load_tasks(&self, heap: &mut BinaryHeap<Reverse<ScheduleEntry>>) {
        let conn = match self.pool.lock() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to lock DB: {e}");
                return;
            }
        };
        let tasks = match task_repo::get_all_enabled_tasks(&conn) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to load tasks: {e}");
                return;
            }
        };
        for task in tasks {
            let next = task.next_run_at
                .filter(|t| *t > Utc::now())
                .or_else(|| self.calculate_next_run(&task));
            if let Some(next) = next {
                if task.next_run_at.is_none() {
                    let _ = task_repo::update_task_run_info(&conn, task.id, None, Some(next), None, None);
                }
                heap.push(Reverse(ScheduleEntry {
                    next_run: next,
                    task_id: task.id,
                }));
                info!("Loaded task '{}' ({}), next run: {}", task.name, task.id, next);
            }
        }
        info!("Loaded {} tasks into scheduler", heap.len());
    }

    fn calculate_next_run(&self, task: &Task) -> Option<DateTime<Utc>> {
        match task.trigger_type {
            TriggerType::Cron => {
                let schedule = Schedule::from_str(&task.trigger_expr).ok()?;
                schedule.after(&Utc::now()).next()
            }
            TriggerType::Once => {
                let dt = DateTime::parse_from_rfc3339(&task.trigger_expr).ok()?;
                let dt_utc = dt.with_timezone(&Utc);
                if dt_utc > Utc::now() {
                    Some(dt_utc)
                } else {
                    None
                }
            }
            TriggerType::Interval => {
                let secs: u64 = task.trigger_expr.parse().ok()?;
                Some(Utc::now() + chrono::Duration::seconds(secs as i64))
            }
        }
    }

    async fn execute_and_reschedule(
        &self,
        heap: &mut BinaryHeap<Reverse<ScheduleEntry>>,
        task_id: Uuid,
    ) {
        let task = {
            let conn = match self.pool.lock() {
                Ok(c) => c,
                Err(e) => {
                    error!("DB lock failed: {e}");
                    return;
                }
            };
            match task_repo::get_task(&conn, task_id) {
                Ok(Some(t)) => t,
                Ok(None) => {
                    warn!("Task {task_id} not found, skipping");
                    return;
                }
                Err(e) => {
                    error!("Failed to get task {task_id}: {e}");
                    return;
                }
            }
        };

        if !task.enabled || task.status != TaskStatus::Active {
            return;
        }

        let task_name = task.name.clone();
        let max_retries = task.max_retries;
        info!("Executing task '{}' ({task_id})", task_name);

        let mut success = false;

        for attempt in 0..=max_retries {
            if attempt > 0 {
                info!("Retrying task '{}' attempt {}/{}", task_name, attempt, max_retries);
            }

            let history = {
                let conn = match self.pool.lock() {
                    Ok(c) => c,
                    Err(e) => {
                        error!("DB lock failed: {e}");
                        return;
                    }
                };
                match history_repo::insert_history(&conn, task_id, RunStatus::Running) {
                    Ok(h) => h,
                    Err(e) => {
                        error!("Failed to create history: {e}");
                        return;
                    }
                }
            };

            let result = executor::execute_task(&task).await;

            {
                let conn = match self.pool.lock() {
                    Ok(c) => c,
                    Err(e) => {
                        error!("DB lock failed: {e}");
                        return;
                    }
                };
                match result {
                    Ok(output) => {
                        let failed = output.exit_code.map_or(false, |c| c != 0);
                        let run_status = if failed { RunStatus::Failed } else { RunStatus::Success };
                        let error_msg = if failed {
                            Some(format!("Command exited with code {}", output.exit_code.unwrap_or(-1)))
                        } else {
                            None
                        };
                        let _ = history_repo::update_history_result(
                            &conn,
                            history.id,
                            run_status,
                            output.exit_code,
                            output.stdout.clone(),
                            output.stderr.clone(),
                            error_msg,
                        );
                        if failed {
                            warn!("Task '{}' failed with exit code {}", task_name, output.exit_code.unwrap_or(-1));
                        } else {
                            success = true;
                            info!("Task '{}' completed successfully", task_name);
                            break;
                        }
                    }
                    Err(e) => {
                        let status = if e.to_string().contains("timeout") {
                            RunStatus::Timeout
                        } else {
                            RunStatus::Failed
                        };
                        let _ = history_repo::update_history_result(
                            &conn,
                            history.id,
                            status,
                            None,
                            None,
                            None,
                            Some(e.to_string()),
                        );
                        warn!("Task '{}' failed: {e}", task_name);
                    }
                }
            }
        }

        {
            let conn = match self.pool.lock() {
                Ok(c) => c,
                Err(e) => {
                    error!("DB lock failed: {e}");
                    return;
                }
            };
            let last_run_status = if success { "success" } else { "failed" };
            let _ = task_repo::update_task_run_info(
                &conn,
                task_id,
                Some(Utc::now()),
                None,
                None,
                Some(last_run_status),
            );

            if let Some(next) = self.calculate_next_run(&task) {
                let _ = task_repo::update_task_run_info(&conn, task_id, None, Some(next), None, None);
                heap.push(Reverse(ScheduleEntry {
                    next_run: next,
                    task_id,
                }));
            } else if task.trigger_type == TriggerType::Once {
                let _ = task_repo::set_task_enabled(&conn, task_id, false);
                let _ = task_repo::update_task_run_info(
                    &conn,
                    task_id,
                    None,
                    None,
                    Some(TaskStatus::Completed),
                    None,
                );
                info!("One-time task '{}' completed and disabled", task_name);
            }
        }
    }

    async fn trigger_task(&self, task_id: Uuid) -> Result<()> {
        let task = {
            let conn = self.pool.lock().map_err(|e| anyhow::anyhow!("Lock failed: {e}"))?;
            task_repo::get_task(&conn, task_id)?
                .ok_or_else(|| anyhow::anyhow!("Task not found"))?
        };

        info!("Manually triggering task '{}'", task.name);

        let history = {
            let conn = self.pool.lock().map_err(|e| anyhow::anyhow!("Lock failed: {e}"))?;
            history_repo::insert_history(&conn, task_id, RunStatus::Running)?
        };

        let result = executor::execute_task(&task).await;

        let conn = self.pool.lock().map_err(|e| anyhow::anyhow!("Lock failed: {e}"))?;
        match result {
            Ok(output) => {
                let failed = output.exit_code.map_or(false, |c| c != 0);
                let status = if failed { RunStatus::Failed } else { RunStatus::Success };
                let error_msg = if failed {
                    Some(format!("Command exited with code {}", output.exit_code.unwrap_or(-1)))
                } else {
                    None
                };
                history_repo::update_history_result(
                    &conn,
                    history.id,
                    status,
                    output.exit_code,
                    output.stdout,
                    output.stderr,
                    error_msg,
                )?;
                let last_run_status = if failed { "failed" } else { "success" };
                task_repo::update_task_run_info(
                    &conn,
                    task_id,
                    Some(Utc::now()),
                    None,
                    None,
                    Some(last_run_status),
                )?;
                if failed {
                    return Err(anyhow::anyhow!("Command exited with code {}", output.exit_code.unwrap_or(-1)));
                }
            }
            Err(e) => {
                history_repo::update_history_result(
                    &conn,
                    history.id,
                    RunStatus::Failed,
                    None,
                    None,
                    None,
                    Some(e.to_string()),
                )?;
                task_repo::update_task_run_info(
                    &conn,
                    task_id,
                    Some(Utc::now()),
                    None,
                    None,
                    Some("failed"),
                )?;
                return Err(e);
            }
        }
        Ok(())
    }
}
