pub mod task_repo;
pub mod history_repo;

use anyhow::Result;
use std::sync::Arc;
use std::sync::Mutex;

pub type DbPool = Arc<Mutex<rusqlite::Connection>>;

pub fn init_db(db_path: &str) -> Result<DbPool> {
    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tasks (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            description     TEXT DEFAULT '',
            trigger_type    TEXT NOT NULL,
            trigger_expr    TEXT NOT NULL,
            action_type     TEXT NOT NULL,
            action_config   TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'active',
            enabled         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL,
            last_run_at     TEXT,
            last_run_status TEXT,
            next_run_at     TEXT,
            max_retries     INTEGER NOT NULL DEFAULT 0,
            timeout_secs    INTEGER,
            gotify_token    TEXT
        )",
    )?;
    // Migration: add last_run_status column if missing
    let has_col: bool = conn.prepare("PRAGMA table_info(tasks)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|name| name.as_deref() == Ok("last_run_status"));
    if !has_col {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN last_run_status TEXT")?;
    }
    // Migration: add gotify_token column if missing
    let has_col: bool = conn.prepare("PRAGMA table_info(tasks)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|name| name.as_deref() == Ok("gotify_token"));
    if !has_col {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN gotify_token TEXT")?;
    }
    // Migration: add UNIQUE constraint on name if missing
    let has_unique: bool = conn.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='tasks'")?
        .query_row([], |row| row.get::<_, String>(0))?
        .contains("UNIQUE");
    if !has_unique {
        conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_name ON tasks(name)")?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS execution_history (
            id          TEXT PRIMARY KEY,
            task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            started_at  TEXT NOT NULL,
            finished_at TEXT,
            status      TEXT NOT NULL,
            exit_code   INTEGER,
            stdout      TEXT,
            stderr      TEXT,
            error_msg   TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0
        )",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_history_task_id ON execution_history(task_id);
         CREATE INDEX IF NOT EXISTS idx_history_started ON execution_history(started_at);",
    )?;
    Ok(Arc::new(Mutex::new(conn)))
}
