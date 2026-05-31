use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Success,
    Failed,
    Timeout,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Success => "success",
            RunStatus::Failed => "failed",
            RunStatus::Timeout => "timeout",
            RunStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(RunStatus::Running),
            "success" => Some(RunStatus::Success),
            "failed" => Some(RunStatus::Failed),
            "timeout" => Some(RunStatus::Timeout),
            "cancelled" => Some(RunStatus::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionHistory {
    pub id: Uuid,
    pub task_id: Uuid,
    pub task_name: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub error_msg: Option<String>,
    pub retry_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct HistoryFilter {
    pub task_id: Option<String>,
    pub task_name: Option<String>,
    pub status: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}
