use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::{
    ActionType, CreateTaskRequest, PagedResult, Task, TaskFilter, TaskStatus, TriggerType,
    UpdateTaskRequest,
};

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
        name: row.get(1)?,
        description: row.get(2)?,
        trigger_type: TriggerType::from_str(&row.get::<_, String>(3)?).unwrap_or(TriggerType::Cron),
        trigger_expr: row.get(4)?,
        action_type: ActionType::from_str(&row.get::<_, String>(5)?).unwrap_or(ActionType::Command),
        action_config: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
        status: TaskStatus::from_str(&row.get::<_, String>(7)?).unwrap_or(TaskStatus::Active),
        enabled: row.get::<_, i32>(8)? != 0,
        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
            .unwrap_or_default()
            .with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(10)?)
            .unwrap_or_default()
            .with_timezone(&Utc),
        last_run_at: row
            .get::<_, Option<String>>(11)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        last_run_status: row.get(12)?,
        next_run_at: row
            .get::<_, Option<String>>(13)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        max_retries: row.get::<_, i64>(14)? as u32,
        timeout_secs: row.get::<_, Option<i64>>(15)?.map(|v| v as u64),
    })
}

pub fn insert_task(conn: &rusqlite::Connection, req: CreateTaskRequest) -> Result<Task> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let enabled = req.enabled.unwrap_or(true);
    let max_retries = req.max_retries.unwrap_or(0) as i32;
    let timeout = req.timeout_secs.map(|v| v as i64);
    conn.execute(
        "INSERT INTO tasks (id, name, description, trigger_type, trigger_expr, action_type, action_config, status, enabled, created_at, updated_at, last_run_status, max_retries, timeout_secs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        rusqlite::params![
            id.to_string(),
            req.name,
            req.description.unwrap_or_default(),
            req.trigger_type.as_str(),
            req.trigger_expr,
            req.action_type.as_str(),
            req.action_config.to_string(),
            TaskStatus::Active.as_str(),
            enabled as i32,
            now,
            now,
            None::<String>,
            max_retries,
            timeout,
        ],
    )?;
    get_task(conn, id)?.ok_or_else(|| anyhow::anyhow!("Failed to retrieve created task"))
}

pub fn get_task(conn: &rusqlite::Connection, id: Uuid) -> Result<Option<Task>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;
    let mut rows = stmt.query_map([id.to_string()], row_to_task)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_tasks(conn: &rusqlite::Connection, filter: TaskFilter) -> Result<PagedResult<Task>> {
    let page = filter.page.unwrap_or(1).max(1);
    let per_page = filter.per_page.unwrap_or(20).min(100);
    let offset = (page - 1) * per_page;

    let mut where_clauses = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref status) = filter.status {
        where_clauses.push(format!("status = ?{idx}"));
        params.push(Box::new(status.clone()));
        idx += 1;
    }
    if let Some(enabled) = filter.enabled {
        where_clauses.push(format!("enabled = ?{idx}"));
        params.push(Box::new(enabled as i32));
        idx += 1;
    }
    if let Some(ref tt) = filter.trigger_type {
        where_clauses.push(format!("trigger_type = ?{idx}"));
        params.push(Box::new(tt.clone()));
        idx += 1;
    }
    if let Some(ref search) = filter.search {
        where_clauses.push(format!("(name LIKE ?{idx} OR description LIKE ?{idx})"));
        params.push(Box::new(format!("%{search}%")));
        idx += 1;
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM tasks {where_sql}");
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        stmt.query_row(params_ref.as_slice(), |row| row.get(0))?
    };

    let query_sql = format!("SELECT * FROM tasks {where_sql} ORDER BY created_at DESC LIMIT ?{idx} OFFSET ?{}", idx + 1);
    let mut stmt = conn.prepare(&query_sql)?;
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = params;
    all_params.push(Box::new(per_page as i64));
    all_params.push(Box::new(offset as i64));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();
    let tasks = stmt
        .query_map(params_ref.as_slice(), row_to_task)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PagedResult {
        items: tasks,
        total: total as u64,
        page,
        per_page,
    })
}

pub fn update_task(conn: &rusqlite::Connection, id: Uuid, req: UpdateTaskRequest) -> Result<Option<Task>> {
    let existing = get_task(conn, id)?;
    if existing.is_none() {
        return Ok(None);
    }

    let now = Utc::now().to_rfc3339();
    let mut sets = vec!["updated_at = ?1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];
    let mut idx = 2;

    if let Some(name) = req.name {
        sets.push(format!("name = ?{idx}"));
        params.push(Box::new(name));
        idx += 1;
    }
    if let Some(desc) = req.description {
        sets.push(format!("description = ?{idx}"));
        params.push(Box::new(desc));
        idx += 1;
    }
    if let Some(tt) = req.trigger_type {
        sets.push(format!("trigger_type = ?{idx}"));
        params.push(Box::new(tt.as_str().to_string()));
        idx += 1;
    }
    if let Some(expr) = req.trigger_expr {
        sets.push(format!("trigger_expr = ?{idx}"));
        params.push(Box::new(expr));
        idx += 1;
    }
    if let Some(at) = req.action_type {
        sets.push(format!("action_type = ?{idx}"));
        params.push(Box::new(at.as_str().to_string()));
        idx += 1;
    }
    if let Some(config) = req.action_config {
        sets.push(format!("action_config = ?{idx}"));
        params.push(Box::new(config.to_string()));
        idx += 1;
    }
    if let Some(enabled) = req.enabled {
        sets.push(format!("enabled = ?{idx}"));
        params.push(Box::new(enabled as i32));
        idx += 1;
    }
    if let Some(max_retries) = req.max_retries {
        sets.push(format!("max_retries = ?{idx}"));
        params.push(Box::new(max_retries));
        idx += 1;
    }
    if let Some(timeout) = req.timeout_secs {
        sets.push(format!("timeout_secs = ?{idx}"));
        params.push(Box::new(timeout as i64));
        idx += 1;
    }

    let sql = format!("UPDATE tasks SET {} WHERE id = ?{idx}", sets.join(", "));
    params.push(Box::new(id.to_string()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, params_ref.as_slice())?;

    get_task(conn, id)
}

pub fn delete_task(conn: &rusqlite::Connection, id: Uuid) -> Result<bool> {
    let rows = conn.execute("DELETE FROM tasks WHERE id = ?1", [id.to_string()])?;
    Ok(rows > 0)
}

pub fn set_task_enabled(conn: &rusqlite::Connection, id: Uuid, enabled: bool) -> Result<Option<Task>> {
    let now = Utc::now().to_rfc3339();
    let status = if enabled { "active" } else { "paused" };
    conn.execute(
        "UPDATE tasks SET enabled = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![enabled as i32, status, now, id.to_string()],
    )?;
    get_task(conn, id)
}

pub fn update_task_run_info(
    conn: &rusqlite::Connection,
    id: Uuid,
    last_run_at: Option<DateTime<Utc>>,
    next_run_at: Option<DateTime<Utc>>,
    status: Option<TaskStatus>,
    last_run_status: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let last = last_run_at.map(|t| t.to_rfc3339());
    let next = next_run_at.map(|t| t.to_rfc3339());
    let status_str = status.map(|s| s.as_str().to_string());
    conn.execute(
        "UPDATE tasks SET last_run_at = COALESCE(?1, last_run_at), next_run_at = COALESCE(?2, next_run_at), status = COALESCE(?3, status), last_run_status = COALESCE(?4, last_run_status), updated_at = ?5 WHERE id = ?6",
        rusqlite::params![last, next, status_str, last_run_status, now, id.to_string()],
    )?;
    Ok(())
}

pub fn get_all_enabled_tasks(conn: &rusqlite::Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE enabled = 1 AND status = 'active'")?;
    let tasks = stmt.query_map([], row_to_task)?.collect::<Result<Vec<_>, _>>()?;
    Ok(tasks)
}
