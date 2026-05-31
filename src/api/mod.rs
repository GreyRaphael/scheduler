pub mod auth;
pub mod tasks;
pub mod history;
pub mod scheduler_control;

use axum::Router;
use tokio::sync::mpsc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;

use crate::db::DbPool;
use crate::scheduler::SchedulerCommand;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub scheduler_tx: mpsc::Sender<SchedulerCommand>,
    pub auth_token: Option<String>,
}

pub fn api_router(state: AppState) -> Router<()> {
    let auth_required = state.auth_token.is_some();

    let tasks_router = tasks::router(state.clone());
    let history_router = history::router(state.clone());
    let scheduler_router = scheduler_control::router(state.clone());
    let auth_router = auth::router(state.clone());

    let mut api = Router::new()
        .nest("/tasks", tasks_router)
        .nest("/history", history_router)
        .nest("/scheduler", scheduler_router)
        .nest("/auth", auth_router);

    if auth_required {
        api = api.layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));
    }

    Router::new()
        .nest("/api/v1", api)
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(CompressionLayer::new())
}
