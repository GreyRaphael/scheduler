use tracing::{debug, info, warn};

pub async fn send_gotify_notification(
    gotify_url: &str,
    token: &str,
    title: &str,
    message: &str,
    priority: u32,
) {
    let client = reqwest::Client::new();
    let url = format!("{}/message?token={}", gotify_url.trim_end_matches('/'), token);

    debug!("Sending Gotify notification to: {}", url);

    let body = serde_json::json!({
        "title": title,
        "message": message,
        "priority": priority,
    });

    match client.post(&url).json(&body).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                info!("Gotify notification sent: {}", title);
            } else {
                let body = resp.text().await.unwrap_or_default();
                warn!("Gotify notification failed with status {status}: {body}");
            }
        }
        Err(e) => {
            warn!("Failed to send Gotify notification: {e}");
        }
    }
}

pub async fn notify_task_result(
    gotify_url: Option<&str>,
    gotify_token: Option<&str>,
    task_name: &str,
    success: bool,
    exit_code: Option<i32>,
) {
    let url = match gotify_url {
        Some(u) if !u.is_empty() => u,
        _ => {
            debug!("Gotify notification skipped: no gotify_url configured");
            return;
        }
    };
    let token = match gotify_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            debug!("Gotify notification skipped: no gotify_token for task '{}'", task_name);
            return;
        }
    };

    let (title, message, priority) = if success {
        (
            format!("Task Success: {}", task_name),
            format!("Task '{}' completed successfully.", task_name),
            5,
        )
    } else {
        let code_str = exit_code.map_or("-".to_string(), |c| c.to_string());
        (
            format!("Task Failed: {}", task_name),
            format!("Task '{}' failed with exit code {}.", task_name, code_str),
            8,
        )
    };

    send_gotify_notification(url, token, &title, &message, priority).await;
}
