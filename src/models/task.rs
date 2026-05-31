use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "cron" => Some(TriggerType::Cron),
            "once" => Some(TriggerType::Once),
            "interval" => Some(TriggerType::Interval),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Command,
    Webhook,
}

impl ActionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionType::Command => "command",
            ActionType::Webhook => "webhook",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "command" => Some(ActionType::Command),
            "webhook" => Some(ActionType::Webhook),
            _ => None,
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

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(TaskStatus::Active),
            "paused" => Some(TaskStatus::Paused),
            "completed" => Some(TaskStatus::Completed),
            "failed" => Some(TaskStatus::Failed),
            _ => None,
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
    pub action_type: ActionType,
    pub action_config: serde_json::Value,
    pub status: TaskStatus,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_status: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub max_retries: u32,
    pub timeout_secs: Option<u64>,
    pub gotify_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger_expr: String,
    pub action_type: ActionType,
    pub action_config: serde_json::Value,
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub gotify_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub trigger_type: Option<TriggerType>,
    pub trigger_expr: Option<String>,
    pub action_type: Option<ActionType>,
    pub action_config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub gotify_token: Option<String>,
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
