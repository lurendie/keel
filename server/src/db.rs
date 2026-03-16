use anyhow::{anyhow, bail, Result};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    auth::{hash_password, verify_password},
    models::{Project, UpsertProjectRequest},
    state::AppState,
    utils::now_ts,
};

pub struct InitDbResult {
    pub generated_admin_password: Option<String>,
}

pub fn init_db(conn: &Connection, admin_password: Option<&str>) -> Result<InitDbResult> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        ",
    )?;

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
            artifact_url TEXT,
            artifact_sha256 TEXT,
            ssh_host TEXT,
            ssh_port INTEGER,
            ssh_user TEXT,
            ssh_private_key_path TEXT,
            ssh_private_key_passphrase TEXT,
            deploy_path TEXT NOT NULL,
            stop_command TEXT,
            start_command TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deploy_mode TEXT NOT NULL DEFAULT 'local'
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

        CREATE INDEX IF NOT EXISTS idx_projects_enabled_branch
            ON projects(enabled, branch);
        CREATE INDEX IF NOT EXISTS idx_deployments_project_id_id
            ON deployments(project_id, id DESC);
        ",
    )?;

    let _ = conn.execute("ALTER TABLE deployments ADD COLUMN log_text TEXT", []);
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN artifact_url TEXT", []);
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN artifact_sha256 TEXT", []);
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN ssh_host TEXT", []);
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN ssh_port INTEGER", []);
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN ssh_user TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE projects ADD COLUMN ssh_private_key_path TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE projects ADD COLUMN ssh_private_key_passphrase TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE projects ADD COLUMN deploy_mode TEXT NOT NULL DEFAULT 'local'",
        [],
    );
    let _ = conn.execute(
        "UPDATE projects
         SET deploy_mode = 'ci_ssh'
         WHERE deploy_mode = 'local'
           AND (
               COALESCE(TRIM(artifact_url), '') <> ''
               OR COALESCE(TRIM(ssh_host), '') <> ''
           )",
        [],
    );

    let generated_admin_password = ensure_admin_password(conn, admin_password)?;

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

    Ok(InitDbResult {
        generated_admin_password,
    })
}

fn ensure_admin_password(
    conn: &Connection,
    admin_password: Option<&str>,
) -> Result<Option<String>> {
    let configured_password = admin_password
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if configured_password.is_some_and(|value| value.len() < 8) {
        bail!("ADMIN_PASSWORD 至少需要 8 位");
    }

    let existing_hash = conn
        .query_row(
            "SELECT password_hash FROM users WHERE username = 'admin'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    if let Some(current_hash) = existing_hash {
        let uses_legacy_default = verify_password("admin123", &current_hash).unwrap_or(false)
            || verify_password("admin", &current_hash).unwrap_or(false);
        if !uses_legacy_default {
            return Ok(None);
        }

        let (password, generated) = match configured_password {
            Some(value) => (value.to_string(), None),
            None => (generate_random_secret(), Some(true)),
        };
        let password_hash = hash_password(&password)?;
        conn.execute(
            "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE username = 'admin'",
            params![password_hash, now_ts()],
        )?;

        return Ok(generated.map(|_| password));
    }

    let (password, generated) = match configured_password {
        Some(value) => (value.to_string(), None),
        None => (generate_random_secret(), Some(true)),
    };
    let password_hash = hash_password(&password)?;
    let now = now_ts();
    conn.execute(
        "INSERT INTO users (id, username, password_hash, created_at, updated_at)
         VALUES (1, 'admin', ?1, ?2, ?2)",
        params![password_hash, now],
    )?;

    Ok(generated.map(|_| password))
}

pub fn get_or_create_jwt_secret(conn: &Connection) -> Result<String> {
    if let Some(existing) = get_setting(conn, "jwt_secret")?.filter(|value| !value.is_empty()) {
        return Ok(existing);
    }

    let secret = generate_random_secret();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('jwt_secret', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![secret],
    )?;
    Ok(secret)
}

fn generate_random_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
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
    if payload.deploy_mode != "local" && payload.deploy_mode != "ci_ssh" {
        bail!("deploy_mode 必须是 local 或 ci_ssh");
    }
    if payload.repo_path.trim().is_empty() {
        bail!("repo_path 不能为空");
    }
    if payload.branch.trim().is_empty() {
        bail!("branch 不能为空");
    }
    if payload.deploy_mode == "local" && payload.build_command.trim().is_empty() {
        bail!("build_command 不能为空");
    }
    if payload.deploy_mode == "local" && payload.built_path.trim().is_empty() {
        bail!("built_path 不能为空");
    }

    if payload.deploy_path.trim().is_empty() {
        bail!("deploy_path 不能为空");
    }

    if payload.deploy_mode == "ci_ssh" {
        if payload
            .ssh_host
            .as_ref()
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            bail!("ssh_host 不能为空");
        }
        if payload
            .ssh_user
            .as_ref()
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            bail!("ssh_user 不能为空");
        }
        if payload
            .ssh_private_key_path
            .as_ref()
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            bail!("ssh_private_key_path 不能为空");
        }
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

pub fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let ssh_private_key_passphrase: Option<String> = row.get(13)?;
    let ssh_private_key_passphrase_set = ssh_private_key_passphrase
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());

    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        repo_path: row.get(2)?,
        branch: row.get(3)?,
        workdir: row.get(4)?,
        build_command: row.get(5)?,
        built_path: row.get(6)?,
        artifact_url: row.get(7)?,
        artifact_sha256: row.get(8)?,
        ssh_host: row.get(9)?,
        ssh_port: row.get(10)?,
        ssh_user: row.get(11)?,
        ssh_private_key_path: row.get(12)?,
        ssh_private_key_passphrase,
        ssh_private_key_passphrase_set,
        deploy_path: row.get(14)?,
        stop_command: row.get(15)?,
        start_command: row.get(16)?,
        enabled: row.get::<_, i64>(17)? == 1,
        created_at: row.get(18)?,
        updated_at: row.get(19)?,
        deploy_mode: row.get(20)?,
    })
}

pub fn get_project_by_id(conn: &Connection, id: i64) -> Result<Option<Project>> {
    let project = conn
        .query_row(
            "SELECT id, name, repo_path, branch, workdir, build_command, built_path, artifact_url, artifact_sha256, ssh_host, ssh_port, ssh_user, ssh_private_key_path, ssh_private_key_passphrase, deploy_path, stop_command, start_command, enabled, created_at, updated_at, deploy_mode
             FROM projects WHERE id = ?1",
            params![id],
            project_from_row,
        )
        .optional()?;
    Ok(project)
}

pub fn list_projects_by_branch(conn: &Connection, branch: &str) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repo_path, branch, workdir, build_command, built_path, artifact_url, artifact_sha256, ssh_host, ssh_port, ssh_user, ssh_private_key_path, ssh_private_key_passphrase, deploy_path, stop_command, start_command, enabled, created_at, updated_at, deploy_mode
         FROM projects
         WHERE enabled = 1 AND branch = ?1
         ORDER BY id DESC",
    )?;

    let rows = stmt.query_map(params![branch], project_from_row)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn local_project() -> UpsertProjectRequest {
        UpsertProjectRequest {
            name: "demo".to_string(),
            deploy_mode: "local".to_string(),
            repo_path: "D:\\repos\\demo".to_string(),
            branch: "main".to_string(),
            workdir: None,
            build_command: "npm run build".to_string(),
            built_path: "dist".to_string(),
            artifact_url: None,
            artifact_sha256: None,
            ssh_host: None,
            ssh_port: None,
            ssh_user: None,
            ssh_private_key_path: None,
            ssh_private_key_passphrase: None,
            deploy_path: "D:\\apps\\demo".to_string(),
            stop_command: None,
            start_command: None,
            enabled: true,
        }
    }

    #[test]
    fn generated_admin_password_replaces_legacy_default() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn, Some("temporary-password")).unwrap();
        conn.execute(
            "UPDATE users SET password_hash = ?1 WHERE username = 'admin'",
            params![hash_password("admin123").unwrap()],
        )
        .unwrap();

        let result = init_db(&conn, None).unwrap();
        let generated = result.generated_admin_password.unwrap();
        let hash: String = conn
            .query_row(
                "SELECT password_hash FROM users WHERE username = 'admin'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(!verify_password("admin123", &hash).unwrap());
        assert!(verify_password(&generated, &hash).unwrap());
    }

    #[test]
    fn configured_admin_password_is_used_for_initialization() {
        let conn = Connection::open_in_memory().unwrap();
        let result = init_db(&conn, Some("strong-password")).unwrap();
        let hash: String = conn
            .query_row(
                "SELECT password_hash FROM users WHERE username = 'admin'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(result.generated_admin_password.is_none());
        assert!(verify_password("strong-password", &hash).unwrap());
    }

    #[test]
    fn rejects_short_bootstrap_password() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(init_db(&conn, Some("short")).is_err());
    }

    #[test]
    fn generated_jwt_secret_is_persisted() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn, Some("strong-password")).unwrap();

        let first = get_or_create_jwt_secret(&conn).unwrap();
        let second = get_or_create_jwt_secret(&conn).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn validates_local_and_remote_project_payloads() {
        let mut project = local_project();
        assert!(validate_project_payload(&project).is_ok());

        project.artifact_url = Some("https://example.com/app.zip".to_string());
        project.deploy_mode = "ci_ssh".to_string();
        assert!(validate_project_payload(&project).is_err());

        project.ssh_host = Some("127.0.0.1".to_string());
        project.ssh_user = Some("deploy".to_string());
        project.ssh_private_key_path = Some("/home/deploy/.ssh/id_ed25519".to_string());
        assert!(validate_project_payload(&project).is_ok());

        project.artifact_url = None;
        assert!(validate_project_payload(&project).is_ok());
    }
}
