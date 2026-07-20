use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::Result;
use chrono::{DateTime, Local, Offset, Utc};
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
    TriggerNow(Uuid),
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
    is_manual: bool,
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

        self.load_tasks(&mut heap).await;

        loop {
            let next_entry = heap.peek().map(|e| e.0.next_run);

            tokio::select! {
                biased;
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        SchedulerCommand::Reload => {
                            info!("Reloading all tasks");
                            heap.clear();
                            self.load_tasks(&mut heap).await;
                        }
                        SchedulerCommand::Pause => {
                            paused = true;
                            info!("Scheduler paused");
                        }
                        SchedulerCommand::Resume => {
                            paused = false;
                            info!("Scheduler resumed");
                            heap.clear();
                            self.load_tasks(&mut heap).await;
                        }
                        SchedulerCommand::TriggerNow(task_id) => {
                            let pool = self.pool.clone();
                            let tx = completion_tx.clone();
                            tokio::spawn(async move {
                                let result = run_task(pool, task_id, true).await;
                                if let Some(comp) = result {
                                    let _ = tx.send(comp).await;
                                }
                            });
                        }
                        SchedulerCommand::Shutdown => {
                            info!("Scheduler shutting down");
                            break;
                        }
                    }
                }
                Some(completion) = completion_rx.recv() => {
                    info!("Task '{}' completed, manual: {}", completion.task_name, completion.is_manual);
                    
                    if !completion.is_manual {
                        let pool = self.pool.clone();
                        let cid = completion.task_id;
                        let next = completion.next_run;
                        let trigger_type = completion.trigger_type.clone();
                        let task_name = completion.task_name.clone();
                        
                        tokio::spawn(async move {
                            if let Ok(conn) = pool.get().await {
                                let _ = conn.interact(move |c| {
                                    if let Some(n) = next {
                                        let _ = task_repo::update_task_run_info(c, cid, None, Some(n), None, None);
                                    } else if trigger_type == TriggerType::Once {
                                        let _ = task_repo::set_task_enabled(c, cid, false);
                                        let _ = task_repo::clear_task_next_run(c, cid);
                                        let _ = task_repo::update_task_run_info(
                                            c, cid, None, None, Some(TaskStatus::Completed), None
                                        );
                                    }
                                }).await;
                            }
                        });
                        
                        if let Some(next) = completion.next_run {
                            heap.push(Reverse(ScheduleEntry {
                                next_run: next,
                                task_id: completion.task_id,
                            }));
                        } else if completion.trigger_type == TriggerType::Once {
                            info!("One-time task '{}' completed and disabled", completion.task_name);
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

    async fn load_tasks(&self, heap: &mut BinaryHeap<Reverse<ScheduleEntry>>) {
        let conn_res = self.pool.get().await;
        if let Err(e) = conn_res {
            error!("Failed to get DB connection: {e}");
            return;
        }
        let conn = conn_res.unwrap();
        
        let (tasks, updates) = match conn.interact(|c| {
            // Also cleanup history occasionally
            let _ = history_repo::cleanup_old_history(c, 1000);
            
            let tasks = task_repo::get_all_enabled_tasks(c)?;
            let mut updates = Vec::new();
            for task in &tasks {
                let next = task.next_run_at
                    .filter(|t| *t > Utc::now())
                    .or_else(|| calculate_next_run_for_task(task));
                if let Some(n) = next {
                    if task.next_run_at != Some(n) {
                        let _ = task_repo::update_task_run_info(c, task.id, None, Some(n), None, None);
                    }
                    updates.push((task.id, task.name.clone(), n));
                }
            }
            Ok::<_, anyhow::Error>((tasks, updates))
        }).await {
            Ok(Ok(res)) => res,
            _ => {
                error!("Failed to load tasks");
                return;
            }
        };

        for (id, name, next) in updates {
            heap.push(Reverse(ScheduleEntry {
                next_run: next,
                task_id: id,
            }));
            info!("Loaded task '{}' ({}), next run: {}", name, id, next);
        }
        info!("Loaded {} tasks into scheduler", heap.len());
    }
}

async fn run_task(
    pool: DbPool,
    task_id: Uuid,
    is_manual: bool,
) -> Option<TaskCompletion> {
    let task = {
        let conn = match pool.get().await {
            Ok(c) => c,
            Err(e) => {
                error!("DB get failed: {e}");
                return None;
            }
        };
        match conn.interact(move |c| task_repo::get_task(c, task_id)).await {
            Ok(Ok(Some(t))) => t,
            Ok(Ok(None)) => {
                warn!("Task {task_id} not found, skipping");
                return None;
            }
            _ => {
                error!("Failed to get task {task_id}");
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
            let conn = match pool.get().await {
                Ok(c) => c,
                Err(e) => {
                    error!("DB get failed: {e}");
                    return None;
                }
            };
            match conn.interact(move |c| history_repo::insert_history(c, task_id, RunStatus::Running)).await {
                Ok(Ok(h)) => h,
                _ => {
                    error!("Failed to create history");
                    return None;
                }
            }
        };

        let result = executor::execute_task(&task).await;

        {
            let conn = match pool.get().await {
                Ok(c) => c,
                Err(e) => {
                    error!("DB get failed: {e}");
                    return None;
                }
            };
            let history_id = history.id;
            
            match result {
                Ok(output) => {
                    let failed = output.exit_code.is_some_and(|c| c != 0);
                    let run_status = if failed { RunStatus::Failed } else { RunStatus::Success };
                    let error_msg = if failed {
                        Some(format!("Command exited with code {}", output.exit_code.unwrap_or(-1)))
                    } else {
                        None
                    };
                    
                    let exit_code = output.exit_code;
                    let stdout = output.stdout;
                    let stderr = output.stderr;
                    
                    let _ = conn.interact(move |c| {
                        history_repo::update_history_result(
                            c,
                            history_id,
                            run_status,
                            exit_code,
                            stdout,
                            stderr,
                            error_msg,
                        )
                    }).await;

                    if failed {
                        warn!("Task '{}' failed with exit code {}", task_name, exit_code.unwrap_or(-1));
                    } else {
                        success = true;
                        info!("Task '{}' completed successfully", task_name);
                        break;
                    }
                }
                Err(e) => {
                    let e_msg = e.to_string();
                    let status = if e_msg.contains("timeout") {
                        RunStatus::Timeout
                    } else {
                        RunStatus::Failed
                    };
                    let _ = conn.interact(move |c| {
                        history_repo::update_history_result(
                            c,
                            history_id,
                            status,
                            None,
                            None,
                            None,
                            Some(e_msg),
                        )
                    }).await;
                    warn!("Task '{}' failed: {e}", task_name);
                }
            }
        }
    }

    // Update task run info
    let next_run = if !is_manual {
        let conn_res = pool.get().await;
        if let Ok(conn) = conn_res {
            let last_run_status = if success { "success".to_string() } else { "failed".to_string() };
            let _ = conn.interact(move |c| {
                task_repo::update_task_run_info(
                    c,
                    task_id,
                    Some(Utc::now()),
                    None,
                    None,
                    Some(&last_run_status),
                )
            }).await;
        }
        calculate_next_run_for_task(&task)
    } else {
        let conn_res = pool.get().await;
        if let Ok(conn) = conn_res {
            let last_run_status = if success { "success".to_string() } else { "failed".to_string() };
            let _ = conn.interact(move |c| {
                task_repo::update_task_run_info(
                    c,
                    task_id,
                    Some(Utc::now()),
                    None,
                    None,
                    Some(&last_run_status),
                )
            }).await;
        }
        None
    };

    if is_manual {
        info!("Manual task '{}' completed", task_name);
    }

        Some(TaskCompletion {
        task_id,
        task_name,
        next_run,
        trigger_type,
        is_manual,
    })
}

fn calculate_next_run_for_task(task: &Task) -> Option<DateTime<Utc>> {
    match task.trigger_type {
        TriggerType::Cron => {
            let tz_offset_secs = if task.cron_tz_mode == "local" {
                Local::now().offset().fix().local_minus_utc()
            } else if task.cron_tz_mode == "utc" {
                0
            } else {
                let tz: chrono_tz::Tz = task.cron_tz_mode.parse().unwrap_or(chrono_tz::UTC);
                let now = Utc::now().with_timezone(&tz);
                now.offset().fix().local_minus_utc()
            };

            let expr = if tz_offset_secs != 0 {
                convert_cron_to_utc(&task.trigger_expr, tz_offset_secs)
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
            if task.interval_mode == "fixed_rate" {
                let base = task.next_run_at.unwrap_or_else(|| Utc::now());
                Some(base + chrono::Duration::seconds(secs as i64))
            } else {
                Some(Utc::now() + chrono::Duration::seconds(secs as i64))
            }
        }
    }
}

fn convert_cron_to_utc(expr: &str, tz_offset_secs: i32) -> String {
    let tz_offset_hours = tz_offset_secs / 3600;
    if tz_offset_hours == 0 {
        return expr.to_string();
    }
    
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 6 {
        return expr.to_string();
    }
    
    let mut new_hour_str = parts[2].to_string();
    if let Ok(h) = parts[2].parse::<i32>() {
        let mut utc_h = h - tz_offset_hours;
        if utc_h < 0 {
            utc_h += 24;
        } else if utc_h >= 24 {
            utc_h -= 24;
        }
        new_hour_str = utc_h.to_string();
    }
    
    format!("{} {} {} {} {} {}", parts[0], parts[1], new_hour_str, parts[3], parts[4], parts[5])
}
