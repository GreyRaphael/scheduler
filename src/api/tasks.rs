use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::AppState;
use crate::models::{CreateTaskRequest, TaskFilter, UpdateTaskRequest};

async fn list_tasks(
    State(state): State<AppState>,
    Query(filter): Query<TaskFilter>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::list_tasks(&conn, filter)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(paged) => Ok(Json(paged)),
        Err(e) => Err(e),
    }
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::get_task(&conn, uuid)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(Some(task)) => Ok(Json(task)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(e),
    }
}

async fn create_task(
    State(state): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::insert_task(&conn, req)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(task) => Ok((StatusCode::CREATED, Json(task))),
        Err(e) => Err(e),
    }
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::update_task(&conn, uuid, req)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(Some(task)) => Ok(Json(task)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(e),
    }
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::delete_task(&conn, uuid)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(e),
    }
}

async fn enable_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::set_task_enabled(&conn, uuid, true)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(Some(task)) => {
            let _ = state.scheduler_tx.try_send(crate::scheduler::SchedulerCommand::Reload);
            Ok(Json(task))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(e),
    }
}

async fn disable_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::task_repo::set_task_enabled(&conn, uuid, false)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(Some(task)) => {
            let _ = state.scheduler_tx.try_send(crate::scheduler::SchedulerCommand::Reload);
            Ok(Json(task))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(e),
    }
}

async fn trigger_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .scheduler_tx
        .try_send(crate::scheduler::SchedulerCommand::TriggerNow(uuid, tx))
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    match rx.await {
        Ok(Ok(())) => Ok(Json(serde_json::json!({"message": "Task triggered successfully"}))),
        Ok(Err(e)) => Ok(Json(serde_json::json!({"error": e.to_string()}))),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route("/{id}", get(get_task).put(update_task).delete(delete_task))
        .route("/{id}/enable", post(enable_task))
        .route("/{id}/disable", post(disable_task))
        .route("/{id}/trigger", post(trigger_task))
}
