use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::error::AppError;
use super::AppState;
use crate::models::{CreateTaskRequest, TaskFilter, UpdateTaskRequest};

async fn list_tasks(
    State(state): State<AppState>,
    Query(filter): Query<TaskFilter>,
) -> Result<impl IntoResponse, AppError> {
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let paged = conn
        .interact(move |c| crate::db::task_repo::list_tasks(c, filter))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;

    Ok(Json(paged))
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(AppError::bad_request)?;
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let task_opt = conn
        .interact(move |c| crate::db::task_repo::get_task(c, uuid))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;

    match task_opt {
        Some(task) => Ok(Json(task)),
        None => Err(AppError::not_found("Task not found")),
    }
}

async fn create_task(
    State(state): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<impl IntoResponse, AppError> {
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let task = conn
        .interact(move |c| crate::db::task_repo::insert_task(c, req))
        .await
        .map_err(AppError::internal)?
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AppError::conflict("Task name already exists")
            } else {
                AppError::internal(e)
            }
        })?;

    let _ = state
        .scheduler_tx
        .send(crate::scheduler::SchedulerCommand::Reload)
        .await;
    Ok((StatusCode::CREATED, Json(task)))
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(AppError::bad_request)?;
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let task_opt = conn
        .interact(move |c| crate::db::task_repo::update_task(c, uuid, req))
        .await
        .map_err(AppError::internal)?
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AppError::conflict("Task name already exists")
            } else {
                AppError::internal(e)
            }
        })?;

    match task_opt {
        Some(task) => {
            let _ = state
                .scheduler_tx
                .send(crate::scheduler::SchedulerCommand::Reload)
                .await;
            Ok(Json(task))
        }
        None => Err(AppError::not_found("Task not found")),
    }
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(AppError::bad_request)?;
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let deleted = conn
        .interact(move |c| crate::db::task_repo::delete_task(c, uuid))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;

    if deleted {
        let _ = state
            .scheduler_tx
            .send(crate::scheduler::SchedulerCommand::Reload)
            .await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("Task not found"))
    }
}

async fn enable_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(AppError::bad_request)?;
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let task_opt = conn
        .interact(move |c| crate::db::task_repo::set_task_enabled(c, uuid, true))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;

    match task_opt {
        Some(task) => {
            let _ = state
                .scheduler_tx
                .send(crate::scheduler::SchedulerCommand::Reload)
                .await;
            Ok(Json(task))
        }
        None => Err(AppError::not_found("Task not found")),
    }
}

async fn disable_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(AppError::bad_request)?;
    let conn = state.pool.get().await.map_err(AppError::internal)?;
    let task_opt = conn
        .interact(move |c| crate::db::task_repo::set_task_enabled(c, uuid, false))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;

    match task_opt {
        Some(task) => {
            let _ = state
                .scheduler_tx
                .send(crate::scheduler::SchedulerCommand::Reload)
                .await;
            Ok(Json(task))
        }
        None => Err(AppError::not_found("Task not found")),
    }
}

async fn trigger_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(AppError::bad_request)?;
    state
        .scheduler_tx
        .send(crate::scheduler::SchedulerCommand::TriggerNow(uuid))
        .await
        .map_err(|_| AppError::unavailable("Scheduler is not available"))?;

    Ok(StatusCode::ACCEPTED)
}

pub fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route("/{id}", get(get_task).put(update_task).delete(delete_task))
        .route("/{id}/enable", post(enable_task))
        .route("/{id}/disable", post(disable_task))
        .route("/{id}/trigger", post(trigger_task))
}
