use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
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

struct TaskCompletion {
    task_id: Uuid,
    task_name: String,
    next_run: Option<DateTime<Utc>>,
    trigger_type: TriggerType,
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
        let (completion_tx, mut completion_rx) = mpsc::channel::<TaskCompletion>(64);

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
                            let pool = self.pool.clone();
                            let tx = completion_tx.clone();
                            tokio::spawn(async move {
                                let result = run_task(pool, task_id, true).await;
                                if let Some(comp) = result {
                                    let _ = tx.send(comp).await;
                                }
                                let _ = reply.send(Ok(()));
                            });
                        }
                        SchedulerCommand::Shutdown => {
                            info!("Scheduler shutting down");
                            break;
                        }
                    }
                }
                Some(completion) = completion_rx.recv() => {
                    info!("Task '{}' completed, rescheduling", completion.task_name);
                    let conn = match self.pool.lock() {
                        Ok(c) => c,
                        Err(e) => {
                            error!("DB lock failed: {e}");
                            continue;
                        }
                    };
                    if let Some(next) = completion.next_run {
                        let _ = task_repo::update_task_run_info(&conn, completion.task_id, None, Some(next), None, None);
                        heap.push(Reverse(ScheduleEntry {
                            next_run: next,
                            task_id: completion.task_id,
                        }));
                    } else if completion.trigger_type == TriggerType::Once {
                        let _ = task_repo::set_task_enabled(&conn, completion.task_id, false);
                        let _ = task_repo::clear_task_next_run(&conn, completion.task_id);
                        let _ = task_repo::update_task_run_info(
                            &conn,
                            completion.task_id,
                            None,
                            None,
                            Some(TaskStatus::Completed),
                            None,
                        );
                        info!("One-time task '{}' completed and disabled", completion.task_name);
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
                            let pool = self.pool.clone();
                            let tx = completion_tx.clone();
                            tokio::spawn(async move {
                                if let Some(comp) = run_task(pool, entry.task_id, false).await {
                                    let _ = tx.send(comp).await;
                                }
                            });
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
                if task.next_run_at != Some(next) {
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
                let expr = if task.cron_tz_mode == "local" {
                    convert_cron_to_utc(&task.trigger_expr)
                } else {
                    task.trigger_expr.clone()
                };
                let schedule = Schedule::from_str(&expr).ok()?;
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
}

async fn run_task(
    pool: DbPool,
    task_id: Uuid,
    is_manual: bool,
) -> Option<TaskCompletion> {
    let task = {
        let conn = match pool.lock() {
            Ok(c) => c,
            Err(e) => {
                error!("DB lock failed: {e}");
                return None;
            }
        };
        match task_repo::get_task(&conn, task_id) {
            Ok(Some(t)) => t,
            Ok(None) => {
                warn!("Task {task_id} not found, skipping");
                return None;
            }
            Err(e) => {
                error!("Failed to get task {task_id}: {e}");
                return None;
            }
        }
    };

    if !task.enabled || task.status != TaskStatus::Active {
        return None;
    }

    let task_name = task.name.clone();
    let max_retries = task.max_retries;
    let trigger_type = task.trigger_type.clone();
    info!("Executing task '{}' ({task_id})", task_name);

    let mut success = false;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            info!("Retrying task '{}' attempt {}/{}", task_name, attempt, max_retries);
        }

        let history = {
            let conn = match pool.lock() {
                Ok(c) => c,
                Err(e) => {
                    error!("DB lock failed: {e}");
                    return None;
                }
            };
            match history_repo::insert_history(&conn, task_id, RunStatus::Running) {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to create history: {e}");
                    return None;
                }
            }
        };

        let result = executor::execute_task(&task).await;

        {
            let conn = match pool.lock() {
                Ok(c) => c,
                Err(e) => {
                    error!("DB lock failed: {e}");
                    return None;
                }
            };
            match result {
                Ok(output) => {
                    let failed = output.exit_code.is_some_and(|c| c != 0);
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

    // Update task run info
    let next_run = {
        let conn = match pool.lock() {
            Ok(c) => c,
            Err(e) => {
                error!("DB lock failed: {e}");
                return None;
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

        calculate_next_run_for_task(&task)
    };

    if is_manual {
        info!("Manual task '{}' completed", task_name);
    }

    Some(TaskCompletion {
        task_id,
        task_name,
        next_run,
        trigger_type,
    })
}

fn calculate_next_run_for_task(task: &Task) -> Option<DateTime<Utc>> {
    match task.trigger_type {
        TriggerType::Cron => {
            let expr = if task.cron_tz_mode == "local" {
                convert_cron_to_utc(&task.trigger_expr)
            } else {
                task.trigger_expr.clone()
            };
            let schedule = Schedule::from_str(&expr).ok()?;
            schedule.after(&Utc::now()).next()
        }
        TriggerType::Once => None,
        TriggerType::Interval => {
            let secs: u64 = task.trigger_expr.parse().ok()?;
            Some(Utc::now() + chrono::Duration::seconds(secs as i64))
        }
    }
}

fn convert_cron_to_utc(expr: &str) -> String {
    let offset_secs = -Local::now().offset().utc_minus_local();
    let offset_hours = offset_secs / 3600;
    if offset_hours == 0 {
        return expr.to_string();
    }
    let fields: Vec<&str> = expr.split_whitespace().collect();
    let hour_idx = if fields.len() == 6 { 2 } else { 1 };
    let mut result: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
    if result[hour_idx] != "*" {
        let shifted: Vec<String> = result[hour_idx]
            .split(',')
            .map(|part| {
                if let Some(step_pos) = part.find('/') {
                    let base = &part[..step_pos];
                    let step = &part[step_pos..];
                    if base == "*" {
                        return format!("*{step}");
                    }
                    if let Some(dash_pos) = base.find('-') {
                        let s: i32 = base[..dash_pos].parse().unwrap_or(0);
                        let e: i32 = base[dash_pos + 1..].parse().unwrap_or(0);
                        let ns = ((s - offset_hours) % 24 + 24) % 24;
                        let ne = ((e - offset_hours) % 24 + 24) % 24;
                        return format!("{ns}-{ne}{step}");
                    }
                    return part.to_string();
                }
                if let Some(dash_pos) = part.find('-') {
                    let s: i32 = part[..dash_pos].parse().unwrap_or(0);
                    let e: i32 = part[dash_pos + 1..].parse().unwrap_or(0);
                    let ns = ((s - offset_hours) % 24 + 24) % 24;
                    let ne = ((e - offset_hours) % 24 + 24) % 24;
                    return format!("{ns}-{ne}");
                }
                if let Ok(h) = part.parse::<i32>() {
                    return format!("{}", ((h - offset_hours) % 24 + 24) % 24);
                }
                part.to_string()
            })
            .collect();
        result[hour_idx] = shifted.join(",");
    }
    result.join(" ")
}
