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
    let conn = state.pool.get().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let paged = conn.interact(move |c| crate::db::history_repo::list_all_history(c, filter))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(paged))
}

async fn clear_all_history(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let conn = state.pool.get().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let count = conn.interact(move |c| crate::db::history_repo::clear_all_history(c))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({"deleted": count})))
}

async fn get_history_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let conn = state.pool.get().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let hist_opt = conn.interact(move |c| crate::db::history_repo::get_history(c, uuid))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match hist_opt {
        Some(h) => Ok(Json(h)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn list_task_history(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Query(filter): Query<HistoryFilter>,
) -> Result<impl IntoResponse, StatusCode> {
    let uuid = Uuid::parse_str(&task_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let conn = state.pool.get().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let paged = conn.interact(move |c| crate::db::history_repo::list_task_history(c, uuid, filter.page, filter.per_page))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(paged))
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
