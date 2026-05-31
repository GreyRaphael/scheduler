use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::AppState;

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    let pool = state.pool.clone();
    let stats = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| anyhow::anyhow!("lock failed"))?;
        let total: i64 =
            conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
        let active: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE enabled = 1 AND status = 'active'",
            [],
            |r| r.get(0),
        )?;
        let paused: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = 'paused'",
            [],
            |r| r.get(0),
        )?;
        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = 'failed'",
            [],
            |r| r.get(0),
        )?;
        let runs_today: i64 = conn.query_row(
            "SELECT COUNT(*) FROM execution_history WHERE started_at >= date('now')",
            [],
            |r| r.get(0),
        )?;
        Ok::<_, anyhow::Error>(serde_json::json!({
            "total_tasks": total,
            "active_tasks": active,
            "paused_tasks": paused,
            "failed_tasks": failed,
            "runs_today": runs_today,
        }))
    })
    .await;

    match stats {
        Ok(Ok(s)) => Json(s).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn pause_scheduler(State(state): State<AppState>) -> impl IntoResponse {
    match state
        .scheduler_tx
        .try_send(crate::scheduler::SchedulerCommand::Pause)
    {
        Ok(()) => Json(serde_json::json!({"message": "Scheduler paused"})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn resume_scheduler(State(state): State<AppState>) -> impl IntoResponse {
    match state
        .scheduler_tx
        .try_send(crate::scheduler::SchedulerCommand::Resume)
    {
        Ok(()) => Json(serde_json::json!({"message": "Scheduler resumed"})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn reload_scheduler(State(state): State<AppState>) -> impl IntoResponse {
    match state
        .scheduler_tx
        .try_send(crate::scheduler::SchedulerCommand::Reload)
    {
        Ok(()) => Json(serde_json::json!({"message": "Scheduler reloaded"})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

pub fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/pause", post(pause_scheduler))
        .route("/resume", post(resume_scheduler))
        .route("/reload", post(reload_scheduler))
}
