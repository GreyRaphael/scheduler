use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    Cron,
    Once,
    Interval,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerType::Cron => "cron",
            TriggerType::Once => "once",
            TriggerType::Interval => "interval",
        }
    }
}

impl fmt::Display for TriggerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TriggerType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cron" => Ok(TriggerType::Cron),
            "once" => Ok(TriggerType::Once),
            "interval" => Ok(TriggerType::Interval),
            _ => Err(format!("Unknown trigger type: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Active,
    Paused,
    Completed,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Active => "active",
            TaskStatus::Paused => "paused",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(TaskStatus::Active),
            "paused" => Ok(TaskStatus::Paused),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            _ => Err(format!("Unknown task status: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub trigger_type: TriggerType,
    pub trigger_expr: String,
    pub cron_tz_mode: String,
    pub command_config: Option<serde_json::Value>,
    pub webhook_config: Option<serde_json::Value>,
    pub status: TaskStatus,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_status: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub max_retries: u32,
    pub timeout_secs: Option<u64>,
    pub interval_mode: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger_expr: String,
    pub cron_tz_mode: Option<String>,
    pub command_config: Option<serde_json::Value>,
    pub webhook_config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub interval_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub trigger_type: Option<TriggerType>,
    pub trigger_expr: Option<String>,
    pub cron_tz_mode: Option<String>,
    pub command_config: Option<serde_json::Value>,
    pub webhook_config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub interval_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TaskFilter {
    pub status: Option<String>,
    pub enabled: Option<bool>,
    pub trigger_type: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PagedResult<T: Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
}
