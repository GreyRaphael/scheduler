use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::error::AppError;
use super::AppState;

async fn status(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let stats = conn
        .interact(move |c| {
            let total: i64 = c.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
            let active: i64 = c.query_row(
                "SELECT COUNT(*) FROM tasks WHERE enabled = 1 AND status = 'active'",
                [],
                |r| r.get(0),
            )?;
            let paused: i64 = c.query_row(
                "SELECT COUNT(*) FROM tasks WHERE enabled = 0",
                [],
                |r| r.get(0),
            )?;
            let failed: i64 = c.query_row(
                "SELECT COUNT(*) FROM execution_history WHERE started_at >= date('now') AND status = 'failed'",
                [],
                |r| r.get(0),
            )?;
            let runs_today: i64 = c.query_row(
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
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;

    Ok(Json(stats))
}

async fn pause_scheduler(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    state
        .scheduler_tx
        .send(crate::scheduler::SchedulerCommand::Pause)
        .await
        .map_err(|_| AppError::unavailable("Scheduler is not available"))?;
    Ok(Json(serde_json::json!({"message": "Scheduler paused"})))
}

async fn resume_scheduler(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    state
        .scheduler_tx
        .send(crate::scheduler::SchedulerCommand::Resume)
        .await
        .map_err(|_| AppError::unavailable("Scheduler is not available"))?;
    Ok(Json(
        serde_json::json!({"message": "Scheduler resumed"}),
    ))
}

async fn reload_scheduler(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    state
        .scheduler_tx
        .send(crate::scheduler::SchedulerCommand::Reload)
        .await
        .map_err(|_| AppError::unavailable("Scheduler is not available"))?;
    Ok(Json(
        serde_json::json!({"message": "Scheduler reloaded"}),
    ))
}

pub fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/pause", post(pause_scheduler))
        .route("/resume", post(resume_scheduler))
        .route("/reload", post(reload_scheduler))
}
