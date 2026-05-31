use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

use crate::models::{ActionType, Task};

#[derive(Debug, Deserialize)]
struct CommandConfig {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    working_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebhookConfig {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
}

fn default_method() -> String {
    "GET".to_string()
}

pub struct TaskOutput {
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

pub async fn execute_task(task: &Task) -> Result<TaskOutput> {
    let timeout = task
        .timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(3600));

    match task.action_type {
        ActionType::Command => execute_command(task, timeout).await,
        ActionType::Webhook => execute_webhook(task, timeout).await,
    }
}

async fn execute_command(task: &Task, timeout: Duration) -> Result<TaskOutput> {
    let config: CommandConfig = serde_json::from_value(task.action_config.clone())
        .context("Failed to parse command action config")?;

    debug!("Running command: {} {:?}", config.program, config.args);

    let mut cmd = Command::new(&config.program);
    cmd.args(&config.args);
    cmd.envs(&config.env);
    if let Some(ref dir) = config.working_dir {
        cmd.current_dir(dir);
    }

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .context("Command timed out")?
        .context("Failed to execute command")?;

    Ok(TaskOutput {
        exit_code: output.status.code(),
        stdout: Some(String::from_utf8_lossy(&output.stdout).to_string()),
        stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
    })
}

async fn execute_webhook(task: &Task, timeout: Duration) -> Result<TaskOutput> {
    let config: WebhookConfig = serde_json::from_value(task.action_config.clone())
        .context("Failed to parse webhook action config")?;

    debug!("Sending webhook: {} {}", config.method, config.url);

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("Failed to build HTTP client")?;

    let method: reqwest::Method = config
        .method
        .parse()
        .context("Invalid HTTP method")?;

    let mut req = client.request(method, &config.url);

    for (k, v) in &config.headers {
        req = req.header(k.as_str(), v.as_str());
    }

    if let Some(body) = config.body {
        req = req.body(body);
    }

    let resp = req.send().await.context("Webhook request failed")?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();

    if status >= 400 {
        anyhow::bail!("Webhook returned status {status}: {body}");
    }

    Ok(TaskOutput {
        exit_code: Some(status as i32),
        stdout: Some(body),
        stderr: None,
    })
}
