use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::{ExecutionHistory, HistoryFilter, PagedResult, RunStatus};

fn row_to_history(row: &rusqlite::Row) -> rusqlite::Result<ExecutionHistory> {
    Ok(ExecutionHistory {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
        task_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
        task_name: row.get(2)?,
        started_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
            .unwrap_or_default()
            .with_timezone(&Utc),
        finished_at: row
            .get::<_, Option<String>>(4)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        status: RunStatus::from_str(&row.get::<_, String>(5)?).unwrap_or(RunStatus::Failed),
        exit_code: row.get(6)?,
        stdout: row.get(7)?,
        stderr: row.get(8)?,
        error_msg: row.get(9)?,
        retry_count: row.get::<_, u32>(10)?,
    })
}

const HISTORY_SELECT: &str = "SELECT h.id, h.task_id, t.name, h.started_at, h.finished_at, h.status, h.exit_code, h.stdout, h.stderr, h.error_msg, h.retry_count FROM execution_history h JOIN tasks t ON h.task_id = t.id";

pub fn insert_history(conn: &rusqlite::Connection, task_id: Uuid, status: RunStatus) -> Result<ExecutionHistory> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO execution_history (id, task_id, started_at, status, retry_count) VALUES (?1, ?2, ?3, ?4, 0)",
        rusqlite::params![id.to_string(), task_id.to_string(), now, status.as_str()],
    )?;
    get_history(conn, id)?.ok_or_else(|| anyhow::anyhow!("Failed to retrieve history"))
}

pub fn update_history_result(
    conn: &rusqlite::Connection,
    id: Uuid,
    status: RunStatus,
    exit_code: Option<i32>,
    stdout: Option<String>,
    stderr: Option<String>,
    error_msg: Option<String>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE execution_history SET finished_at = ?1, status = ?2, exit_code = ?3, stdout = ?4, stderr = ?5, error_msg = ?6 WHERE id = ?7",
        rusqlite::params![now, status.as_str(), exit_code, stdout, stderr, error_msg, id.to_string()],
    )?;
    Ok(())
}

pub fn get_history(conn: &rusqlite::Connection, id: Uuid) -> Result<Option<ExecutionHistory>> {
    let sql = format!("{HISTORY_SELECT} WHERE h.id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([id.to_string()], row_to_history)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_task_history(
    conn: &rusqlite::Connection,
    task_id: Uuid,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<PagedResult<ExecutionHistory>> {
    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).min(100);
    let offset = (page - 1) * per_page;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM execution_history WHERE task_id = ?1",
        [task_id.to_string()],
        |row| row.get(0),
    )?;

    let sql = format!("{HISTORY_SELECT} WHERE h.task_id = ?1 ORDER BY h.started_at DESC LIMIT ?2 OFFSET ?3");
    let mut stmt = conn.prepare(&sql)?;
    let history = stmt
        .query_map(
            rusqlite::params![task_id.to_string(), per_page as i64, offset as i64],
            row_to_history,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PagedResult {
        items: history,
        total: total as u64,
        page,
        per_page,
    })
}

pub fn list_all_history(conn: &rusqlite::Connection, filter: HistoryFilter) -> Result<PagedResult<ExecutionHistory>> {
    let page = filter.page.unwrap_or(1).max(1);
    let per_page = filter.per_page.unwrap_or(20).min(100);
    let offset = (page - 1) * per_page;

    let mut where_clauses = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref task_id) = filter.task_id {
        where_clauses.push(format!("h.task_id = ?{idx}"));
        params.push(Box::new(task_id.clone()));
        idx += 1;
    }
    if let Some(ref task_name) = filter.task_name {
        where_clauses.push(format!("t.name LIKE ?{idx}"));
        params.push(Box::new(format!("%{task_name}%")));
        idx += 1;
    }
    if let Some(ref status) = filter.status {
        where_clauses.push(format!("h.status = ?{idx}"));
        params.push(Box::new(status.clone()));
        idx += 1;
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM execution_history h JOIN tasks t ON h.task_id = t.id {where_sql}");
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        stmt.query_row(params_ref.as_slice(), |row| row.get(0))?
    };

    let query_sql = format!(
        "{HISTORY_SELECT} {where_sql} ORDER BY h.started_at DESC LIMIT ?{idx} OFFSET ?{}",
        idx + 1
    );
    let mut stmt = conn.prepare(&query_sql)?;
    params.push(Box::new(per_page as i64));
    params.push(Box::new(offset as i64));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let history = stmt
        .query_map(params_ref.as_slice(), row_to_history)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PagedResult {
        items: history,
        total: total as u64,
        page,
        per_page,
    })
}
