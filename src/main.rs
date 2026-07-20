mod api;
mod config;
mod db;
mod models;
mod scheduler;

use anyhow::Result;
use axum::response::IntoResponse;
use axum::Router;
use rust_embed::Embed;
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use api::AppState;
use config::Config;

#[derive(Embed)]
#[folder = "src/web/static/"]
struct Assets;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(config.log_level_str())),
        )
        .init();

    info!("Starting scheduler on {}", config.listen_addr());

    let db_path = config.db_path().to_string_lossy().to_string();
    let pool = db::init_db(&db_path)?;
    info!("Database initialized: {db_path}");

    let engine = scheduler::SchedulerEngine::new(pool.clone());
    let scheduler_tx = engine.command_sender();

    let scheduler_handle = tokio::spawn(async move {
        engine.run().await;
    });

    let state = AppState {
        pool: pool.clone(),
        scheduler_tx: scheduler_tx.clone(),
        auth_token: config.token.clone(),
    };

    let api_routes = api::api_router(state.clone());

    let static_routes = Router::new()
        .fallback(static_handler);

    let app = Router::new()
        .merge(api_routes)
        .merge(static_routes)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(config.listen_addr()).await?;
    info!("Listening on {}", config.listen_addr());

    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                warn!("Server error: {e}");
            }
        }
        _ = scheduler_handle => {
            warn!("Scheduler engine exited unexpectedly");
        }
    }

    let _ = scheduler_tx.send(scheduler::SchedulerCommand::Shutdown).await;
    info!("Shutdown complete");
    Ok(())
}

async fn static_handler(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let is_index = path.is_empty() || path == "index.html";

    let file = if is_index {
        Assets::get("index.html")
    } else {
        Assets::get(path)
    };

    match file {
        Some(content) => {
            let mime = if is_index {
                "text/html".to_string()
            } else {
                mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string()
            };
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, mime)],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => {
            if let Some(content) = Assets::get("index.html") {
                (
                    axum::http::StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "text/html".to_string())],
                    content.data.to_vec(),
                )
                    .into_response()
            } else {
                axum::http::StatusCode::NOT_FOUND.into_response()
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}
