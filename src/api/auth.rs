use axum::extract::{Request, State};
use axum::http::header;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use super::AppState;

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();

    if path.ends_with("/auth/login") || path.ends_with("/auth/check") {
        return Ok(next.run(req).await);
    }

    if let Some(expected) = &state.auth_token {
        let auth_header = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(token_header) = auth_header
            && let Some(token) = token_header.strip_prefix("Bearer ")
            && token == expected.as_str()
        {
            return Ok(next.run(req).await);
        }

        if let Some(cookie) = req.headers().get(header::COOKIE).and_then(|v| v.to_str().ok()) {
            for part in cookie.split(';') {
                let part = part.trim();
                if let Some(val) = part.strip_prefix("auth_token=")
                    && val == expected.as_str()
                {
                    return Ok(next.run(req).await);
                }
            }
        }

        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

#[derive(serde::Deserialize)]
pub struct LoginBody {
    pub token: String,
}

async fn login(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<LoginBody>,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    match &state.auth_token {
        Some(expected) if body.token == *expected => Ok(axum::Json(serde_json::json!({
            "authenticated": true,
            "message": "Login successful"
        }))),
        Some(_) => Err(StatusCode::UNAUTHORIZED),
        None => Ok(axum::Json(serde_json::json!({
            "authenticated": true,
            "message": "Auth not configured"
        }))),
    }
}

async fn check(State(state): State<AppState>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "auth_required": state.auth_token.is_some()
    }))
}

pub fn router(_state: AppState) -> axum::Router<AppState> {
    axum::Router::new()
        .route("/login", axum::routing::post(login))
        .route("/check", axum::routing::get(check))
}
