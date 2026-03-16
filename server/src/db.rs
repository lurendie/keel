use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    auth::hash_password,
    models::{Project, UpsertProjectRequest},
    state::AppState,
    utils::now_ts,
};

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            repo_path TEXT NOT NULL,
            branch TEXT NOT NULL,
            workdir TEXT,
            build_command TEXT NOT NULL,
            built_path TEXT NOT NULL,
            deploy_path TEXT NOT NULL,
            stop_command TEXT,
            start_command TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS deployments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            branch TEXT NOT NULL,
            status TEXT NOT NULL,
            message TEXT,
            log_text TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            FOREIGN KEY(project_id) REFERENCES projects(id)
        );
        ",
    )?;

    let _ = conn.execute("ALTER TABLE deployments ADD COLUMN log_text TEXT", []);

    let now = now_ts();
    let default_hash = hash_password("admin")?;
    conn.execute(
        "INSERT OR IGNORE INTO users (id, username, password_hash, created_at, updated_at)
         VALUES (1, 'admin', ?1, ?2, ?2)",
        params![default_hash, now],
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('webhook_bind', '0.0.0.0:8000')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('webhook_path', '/webhook')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('webhook_secret', '')",
        [],
    )?;

    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let value = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(value)
}

pub fn validate_project_payload(payload: &UpsertProjectRequest) -> Result<()> {
    if payload.name.trim().is_empty() {
        bail!("项目名称不能为空");
    }
    if payload.repo_path.trim().is_empty() {
        bail!("repo_path 不能为空");
    }
    if payload.branch.trim().is_empty() {
        bail!("branch 不能为空");
    }
    if payload.build_command.trim().is_empty() {
        bail!("build_command 不能为空");
    }
    if payload.built_path.trim().is_empty() {
        bail!("built_path 不能为空");
    }
    if payload.deploy_path.trim().is_empty() {
        bail!("deploy_path 不能为空");
    }
    Ok(())
}

pub fn insert_deployment(
    conn: &Connection,
    project_id: i64,
    branch: &str,
    status: &str,
    message: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO deployments (project_id, branch, status, message, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![project_id, branch, status, message, now_ts()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_project_by_id(conn: &Connection, id: i64) -> Result<Option<Project>> {
    let project = conn
        .query_row(
            "SELECT id, name, repo_path, branch, workdir, build_command, built_path, deploy_path, stop_command, start_command, enabled, created_at, updated_at
             FROM projects WHERE id = ?1",
            params![id],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    repo_path: row.get(2)?,
                    branch: row.get(3)?,
                    workdir: row.get(4)?,
                    build_command: row.get(5)?,
                    built_path: row.get(6)?,
                    deploy_path: row.get(7)?,
                    stop_command: row.get(8)?,
                    start_command: row.get(9)?,
                    enabled: row.get::<_, i64>(10)? == 1,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            },
        )
        .optional()?;
    Ok(project)
}

pub fn list_projects_by_branch(conn: &Connection, branch: &str) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repo_path, branch, workdir, build_command, built_path, deploy_path, stop_command, start_command, enabled, created_at, updated_at
         FROM projects
         WHERE enabled = 1 AND branch = ?1
         ORDER BY id DESC",
    )?;

    let rows = stmt.query_map(params![branch], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            repo_path: row.get(2)?,
            branch: row.get(3)?,
            workdir: row.get(4)?,
            build_command: row.get(5)?,
            built_path: row.get(6)?,
            deploy_path: row.get(7)?,
            stop_command: row.get(8)?,
            start_command: row.get(9)?,
            enabled: row.get::<_, i64>(10)? == 1,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    })?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row?);
    }
    Ok(list)
}

pub fn update_deployment_status(
    state: &AppState,
    deploy_id: i64,
    status: &str,
    message: Option<String>,
    log_text: Option<String>,
) -> Result<()> {
    let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
    conn.execute(
        "UPDATE deployments
         SET status = ?1,
             message = COALESCE(?2, message),
             log_text = COALESCE(?3, log_text),
             finished_at = CASE WHEN ?1 IN ('success', 'failed') THEN ?4 ELSE finished_at END
         WHERE id = ?5",
        params![status, message, log_text, now_ts(), deploy_id],
    )?;
    Ok(())
}
