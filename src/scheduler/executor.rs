use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

use crate::models::Task;

#[derive(Debug, Deserialize)]
pub struct CommandConfig {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_method() -> String {
    "GET".to_string()
}

pub struct TaskOutput {
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

pub struct TaskContext {
    pub task_name: String,
    pub cmd_output: Option<TaskOutput>,
}

impl TaskOutput {
    pub fn status_str(&self) -> &str {
        if self.exit_code.is_some_and(|c| c == 0) { "success" } else { "failed" }
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\u0000"),
            c if c < '\x20' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn render_template(template: &str, ctx: &TaskContext, is_json: bool) -> String {
    let mut result = template.to_string();
    
    let escape = |s: &str| -> String {
        if is_json {
            json_escape(s)
        } else {
            s.to_string()
        }
    };

    if let Some(ref cmd) = ctx.cmd_output {
        result = result.replace("{{exit_code}}", &cmd.exit_code.map_or("-".to_string(), |c| c.to_string()));
        result = result.replace("{{stdout}}", &escape(cmd.stdout.as_deref().unwrap_or("")));
        result = result.replace("{{stderr}}", &escape(cmd.stderr.as_deref().unwrap_or("")));
        result = result.replace("{{status}}", cmd.status_str());
    } else {
        result = result.replace("{{exit_code}}", "-");
        result = result.replace("{{stdout}}", "");
        result = result.replace("{{stderr}}", "");
        result = result.replace("{{status}}", "success");
    }
    result = result.replace("{{task_name}}", &escape(&ctx.task_name));
    result
}

pub async fn execute_task(task: &Task) -> Result<TaskOutput> {
    let timeout = task
        .timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(3600));

    let mut ctx = TaskContext {
        task_name: task.name.clone(),
        cmd_output: None,
    };

    // Run command if configured
    if let Some(ref cmd_config) = task.command_config {
        ctx.cmd_output = Some(execute_command(cmd_config, timeout).await?);
    }

    // Run webhook if configured (with command result context)
    if let Some(ref wh_config) = task.webhook_config {
        match execute_webhook(wh_config, timeout, &ctx).await {
            Ok(wh_out) => {
                if ctx.cmd_output.is_none() {
                    return Ok(wh_out);
                }
            }
            Err(e) => {
                if ctx.cmd_output.is_some() {
                    tracing::warn!("Webhook failed but command succeeded: {}", e);
                } else {
                    return Err(e);
                }
            }
        }
    }

    // Return command result if available
    if let Some(output) = ctx.cmd_output {
        Ok(output)
    } else {
        anyhow::bail!("No command or webhook configured for task");
    }
}

async fn execute_command(config_val: &serde_json::Value, timeout: Duration) -> Result<TaskOutput> {
    let config: CommandConfig = serde_json::from_value(config_val.clone())
        .context("Failed to parse command config")?;

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

async fn execute_webhook(config_val: &serde_json::Value, timeout: Duration, ctx: &TaskContext) -> Result<TaskOutput> {
    let config: WebhookConfig = serde_json::from_value(config_val.clone())
        .context("Failed to parse webhook config")?;

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

    let mut is_json = true;
    let mut has_ct = false;
    for (k, v) in &config.headers {
        req = req.header(k.as_str(), v.as_str());
        if k.eq_ignore_ascii_case("content-type") {
            has_ct = true;
            if !v.to_lowercase().contains("application/json") {
                is_json = false;
            }
        }
    }

    if let Some(body) = config.body {
        let rendered = render_template(&body, ctx, is_json);
        if !has_ct {
            req = req.header("Content-Type", "application/json");
        }
        req = req.body(rendered);
    }

    let resp = req.send().await.context("Webhook request failed")?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();

    if status >= 400 {
        anyhow::bail!("Webhook returned status {status}: {body}");
    }

    Ok(TaskOutput {
        exit_code: Some(0),
        stdout: Some(body),
        stderr: None,
    })
}
