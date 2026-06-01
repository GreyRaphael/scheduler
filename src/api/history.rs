use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::AppState;
use crate::models::HistoryFilter;

async fn list_all_history(
    State(state): State<AppState>,
    Query(filter): Query<HistoryFilter>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::history_repo::list_all_history(&conn, filter)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(paged) => Ok(Json(paged)),
        Err(e) => Err(e),
    }
}

async fn clear_all_history(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::history_repo::clear_all_history(&conn)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(count) => Ok(Json(serde_json::json!({"deleted": count}))),
        Err(e) => Err(e),
    }
}

async fn get_history_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::history_repo::get_history(&conn, uuid)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(Some(h)) => Ok(Json(h)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(e),
    }
}

async fn list_task_history(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Query(filter): Query<HistoryFilter>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&task_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pool = state.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::db::history_repo::list_task_history(&conn, uuid, filter.page, filter.per_page)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok(paged) => Ok(Json(paged)),
        Err(e) => Err(e),
    }
}

pub fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/", get(list_all_history).delete(clear_all_history))
        .route("/{id}", get(get_history_detail))
        .nest(
            "/task/{task_id}",
            Router::new().route("/", get(list_task_history)),
        )
}
