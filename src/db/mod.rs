pub mod task_repo;
pub mod history_repo;

use anyhow::Result;
use deadpool_sqlite::{Config, Pool, Runtime};

pub type DbPool = Pool;

pub fn init_db(db_path: &str) -> Result<DbPool> {
    let mut cfg = Config::new(db_path);
    let pool = cfg.create_pool(Runtime::Tokio1)?;
    
    // We run schema init synchronously since it's startup
    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tasks (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            description     TEXT DEFAULT '',
            trigger_type    TEXT NOT NULL,
            trigger_expr    TEXT NOT NULL,
            command_config  TEXT,
            webhook_config  TEXT,
            status          TEXT NOT NULL DEFAULT 'active',
            enabled         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL,
            last_run_at     TEXT,
            last_run_status TEXT,
            next_run_at     TEXT,
            max_retries     INTEGER NOT NULL DEFAULT 0,
            timeout_secs    INTEGER,
            cron_tz_mode    TEXT NOT NULL DEFAULT 'utc',
            interval_mode   TEXT NOT NULL DEFAULT 'fixed_delay'
        )",
    )?;

    // Migration: check if old schema (has action_type column)
    let has_action_type: bool = conn.prepare("PRAGMA table_info(tasks)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|name| name.as_deref() == Ok("action_type"));

    if has_action_type {
        // Migrate: add new columns, copy data, recreate table
        let has_cmd: bool = conn.prepare("PRAGMA table_info(tasks)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .any(|name| name.as_deref() == Ok("command_config"));
        if !has_cmd {
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN command_config TEXT")?;
        }
        let has_wh: bool = conn.prepare("PRAGMA table_info(tasks)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .any(|name| name.as_deref() == Ok("webhook_config"));
        if !has_wh {
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN webhook_config TEXT")?;
        }

        // Migrate existing data
        {
            let mut stmt = conn.prepare("SELECT id, action_type, action_config, gotify_token FROM tasks")?;
            let rows: Vec<(String, String, String, Option<String>)> = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?.collect::<Result<Vec<_>, _>>()?;

            for (id, action_type, action_config, gotify_token) in rows {
                let (final_cmd, final_wh) = match action_type.as_str() {
                    "command" => (Some(action_config.clone()), gotify_token_webhook(&gotify_token)),
                    _ => (None, Some(action_config.clone())),
                };
                conn.execute(
                    "UPDATE tasks SET command_config = ?1, webhook_config = ?2 WHERE id = ?3",
                    rusqlite::params![final_cmd, final_wh, id],
                )?;
            }
        }

        // Recreate table without old columns
        conn.execute_batch("
            CREATE TABLE tasks_new (
                id              TEXT PRIMARY KEY,
                name            TEXT NOT NULL UNIQUE,
                description     TEXT DEFAULT '',
                trigger_type    TEXT NOT NULL,
                trigger_expr    TEXT NOT NULL,
                command_config  TEXT,
                webhook_config  TEXT,
                status          TEXT NOT NULL DEFAULT 'active',
                enabled         INTEGER NOT NULL DEFAULT 1,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                last_run_at     TEXT,
                last_run_status TEXT,
                next_run_at     TEXT,
                max_retries     INTEGER NOT NULL DEFAULT 0,
                timeout_secs    INTEGER,
                cron_tz_mode    TEXT NOT NULL DEFAULT 'utc'
            )
        ")?;
        conn.execute_batch("
            INSERT INTO tasks_new (id, name, description, trigger_type, trigger_expr, command_config, webhook_config, status, enabled, created_at, updated_at, last_run_at, last_run_status, next_run_at, max_retries, timeout_secs, cron_tz_mode)
            SELECT id, name, description, trigger_type, trigger_expr, command_config, webhook_config, status, enabled, created_at, updated_at, last_run_at, last_run_status, next_run_at, max_retries, timeout_secs, COALESCE(cron_tz_mode, 'utc')
            FROM tasks
        ")?;
        conn.execute_batch("DROP TABLE tasks")?;
        conn.execute_batch("ALTER TABLE tasks_new RENAME TO tasks")?;
    } else {
        // New schema already, ensure UNIQUE on name
        let has_unique: bool = conn.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='tasks'")?
            .query_row([], |row| row.get::<_, String>(0))?
            .contains("UNIQUE");
        if !has_unique {
            conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_name ON tasks(name)")?;
        }
        
        let has_interval_mode: bool = conn.prepare("PRAGMA table_info(tasks)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .any(|name| name.as_deref() == Ok("interval_mode"));
        if !has_interval_mode {
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN interval_mode TEXT NOT NULL DEFAULT 'fixed_delay'")?;
        }
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
    Ok(pool)
}

fn gotify_token_webhook(token: &Option<String>) -> Option<String> {
    token.as_ref().filter(|t| !t.is_empty()).map(|t| {
        serde_json::json!({
            "url": format!("http://localhost:8080/message?token={}", t),
            "method": "POST",
            "body": serde_json::json!({
                "title": "{{task_name}}",
                "message": "Task {{status}}",
                "priority": 5
            }).to_string()
        }).to_string()
    })
}
