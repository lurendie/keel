use anyhow::{anyhow, bail, Result};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

use crate::{
    auth::{hash_password, verify_password},
    models::{DeployTarget, Project, Server, ServerSummary, UpsertProjectRequest, UpsertServerRequest},
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
            updated_at TEXT NOT NULL,
            last_login_ip TEXT,
            last_login_at TEXT
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

        CREATE TABLE IF NOT EXISTS servers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            group_name TEXT,
            host TEXT NOT NULL,
            port INTEGER NOT NULL DEFAULT 22,
            username TEXT NOT NULL,
            auth_method TEXT NOT NULL DEFAULT 'key',
            private_key_path TEXT,
            private_key_passphrase TEXT,
            password TEXT,
            deploy_path TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        ",
    )?;

    let _ = conn.execute("ALTER TABLE deployments ADD COLUMN log_text TEXT", []);
    let _ = conn.execute("ALTER TABLE users ADD COLUMN last_login_ip TEXT", []);
    let _ = conn.execute("ALTER TABLE users ADD COLUMN last_login_at TEXT", []);
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
    let _ = conn.execute(
        "UPDATE projects
         SET artifact_url = NULL, artifact_sha256 = NULL",
        [],
    );

    migrate_ssh_fields_to_servers(conn)?;

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

/// 把历史版本里写在 projects 上的 SSH 配置收敛成 servers 记录。
///
/// 幂等：只处理 `server_id IS NULL` 且填了 host 的项目，重复启动不会产生重复服务器。
/// 判定重复用 `IS`（NULL 安全）比较，否则 NULL = NULL 永远不成立，每次启动都会插一条。
fn migrate_ssh_fields_to_servers(conn: &Connection) -> Result<()> {
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN server_id INTEGER", []);

    let mut stmt = conn.prepare(
        "SELECT id, name, ssh_host, ssh_port, ssh_user, ssh_private_key_path, ssh_private_key_passphrase
         FROM projects
         WHERE server_id IS NULL
           AND COALESCE(TRIM(ssh_host), '') <> ''
         ORDER BY id",
    )?;
    let rows: Vec<(i64, String, String, Option<i64>, Option<String>, Option<String>, Option<String>)> =
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    for (project_id, project_name, host, port, user, key_path, passphrase) in rows {
        let port = port.filter(|value| *value > 0).unwrap_or(22);
        let user = user.unwrap_or_else(|| "root".to_string());

        // 同一台机器被多个项目引用时复用同一条服务器记录
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM servers
                 WHERE host = ?1 AND port = ?2 AND username = ?3
                   AND private_key_path IS ?4
                   AND private_key_passphrase IS ?5
                 ORDER BY id LIMIT 1",
                params![host, port, user, key_path, passphrase],
                |row| row.get(0),
            )
            .optional()?;

        let server_id = match existing {
            Some(id) => id,
            None => {
                conn.execute(
                    "INSERT INTO servers
                     (name, group_name, host, port, username, auth_method, private_key_path,
                      private_key_passphrase, password, deploy_path, enabled, created_at, updated_at)
                     VALUES (?1, NULL, ?2, ?3, ?4, 'key', ?5, ?6, NULL, NULL, 1, ?7, ?7)",
                    params![project_name, host, port, user, key_path, passphrase, now_ts()],
                )?;
                conn.last_insert_rowid()
            }
        };

        conn.execute(
            "UPDATE projects SET server_id = ?1 WHERE id = ?2",
            params![server_id, project_id],
        )?;
    }

    Ok(())
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
    validate_deploy_path(payload.deploy_path.trim(), &payload.deploy_mode)?;

    // ci_ssh 必须挂一台服务器：没有它就没有主机、用户和凭据，部署一定失败。
    if payload.deploy_mode == "ci_ssh" && payload.server_id.is_none() {
        bail!("CI + SSH 部署需要先选择一台服务器");
    }

    Ok(())
}

/// 服务器配置的校验。认证方式决定哪些凭据是必需的。
pub fn validate_server_payload(payload: &UpsertServerRequest) -> Result<()> {
    if payload.name.trim().is_empty() {
        bail!("服务器名称不能为空");
    }
    if payload.host.trim().is_empty() {
        bail!("主机地址不能为空");
    }
    if payload.username.trim().is_empty() {
        bail!("登录用户不能为空");
    }
    if !(1..=65535).contains(&payload.port) {
        bail!("端口必须在 1-65535 之间");
    }
    match payload.auth_method.as_str() {
        "key" => {
            let has_key = payload
                .private_key_path
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !has_key {
                bail!("使用密钥认证时必须填写私钥路径");
            }
        }
        "password" => {
            // 更新时允许留空沿用已保存的密码，所以这里只拦「创建时也没给」的情况。
            // 调用方在 update 分支会先读旧记录再判断，这里保持宽松。
        }
        other => bail!("auth_method 必须是 key 或 password，收到 {other}"),
    }

    if let Some(path) = payload.deploy_path.as_deref() {
        let path = path.trim();
        if !path.is_empty() && !path.starts_with('/') {
            bail!("远程部署目录必须是以 / 开头的绝对路径");
        }
    }

    Ok(())
}
/// Local deployments delete `deploy_path` before copying, so refuse anything
/// that could resolve to a filesystem root or a relative location.
fn validate_deploy_path(value: &str, deploy_mode: &str) -> Result<()> {
    if deploy_mode == "ci_ssh" {
        if !value.starts_with('/') || value.trim_end_matches('/').is_empty() {
            bail!("远程 deploy_path 必须是以 / 开头的绝对路径");
        }
        return Ok(());
    }

    let path = Path::new(value);
    if !path.is_absolute() {
        bail!("deploy_path 必须是绝对路径，例如 /srv/apps/demo");
    }
    if path.parent().is_none() || path.file_name().is_none() {
        bail!("deploy_path 不能是文件系统根目录");
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

/// A deployment cannot survive a process restart, so anything left queued or
/// running is reported as failed instead of staying "in progress" forever.
pub fn fail_interrupted_deployments(conn: &Connection) -> Result<usize> {
    let affected = conn.execute(
        "UPDATE deployments
         SET status = 'failed',
             message = COALESCE(message || ' | ', '') || '服务重启导致部署中断',
             finished_at = ?1
         WHERE status IN ('queued', 'running')",
        params![now_ts()],
    )?;
    Ok(affected)
}

/// 项目的统一投影。SSH 连接字段全部来自 servers 表，`p.ssh_*` 只作为
/// 尚未迁移完成时的兜底，`COALESCE` 让迁移前后行为一致。
pub const PROJECT_SELECT: &str = "SELECT p.id, p.name, p.repo_path, p.branch, p.workdir, p.build_command,
            p.built_path, p.artifact_url, p.artifact_sha256,
            p.server_id, s.name,
            COALESCE(s.host, p.ssh_host), COALESCE(s.port, p.ssh_port), COALESCE(s.username, p.ssh_user),
            COALESCE(s.private_key_path, p.ssh_private_key_path),
            COALESCE(s.private_key_passphrase, p.ssh_private_key_passphrase),
            COALESCE(s.auth_method, 'key'), s.password,
            p.deploy_path, p.stop_command, p.start_command, p.enabled, p.created_at, p.updated_at, p.deploy_mode
     FROM projects p
     LEFT JOIN servers s ON s.id = p.server_id";

pub fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let ssh_private_key_passphrase: Option<String> = row.get(15)?;
    let ssh_private_key_passphrase_set = ssh_private_key_passphrase
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let ssh_password: Option<String> = row.get(17)?;
    let ssh_password_set = ssh_password
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
        server_id: row.get(9)?,
        server_name: row.get(10)?,
        ssh_host: row.get(11)?,
        ssh_port: row.get(12)?,
        ssh_user: row.get(13)?,
        ssh_private_key_path: row.get(14)?,
        ssh_private_key_passphrase,
        ssh_auth_method: row.get(16)?,
        ssh_password,
        ssh_private_key_passphrase_set,
        ssh_password_set,
        deploy_path: row.get(18)?,
        stop_command: row.get(19)?,
        start_command: row.get(20)?,
        enabled: row.get::<_, i64>(21)? == 1,
        created_at: row.get(22)?,
        updated_at: row.get(23)?,
        deploy_mode: row.get(24)?,
    })
}

/// 取出部署阶段需要的 SSH 配置。缺服务器或缺字段时返回 None，
/// 由调用方给出「未选择服务器」这类可读错误。
pub fn deploy_target_for_project(project: &Project) -> Option<DeployTarget> {
    let host = project.ssh_host.as_deref()?.trim();
    let user = project.ssh_user.as_deref()?.trim();
    if host.is_empty() || user.is_empty() {
        return None;
    }

    Some(DeployTarget {
        server_id: project.server_id.unwrap_or(0),
        name: project
            .server_name
            .clone()
            .unwrap_or_else(|| host.to_string()),
        host: host.to_string(),
        port: project.ssh_port.filter(|value| *value > 0).unwrap_or(22),
        user: user.to_string(),
        auth_method: project
            .ssh_auth_method
            .clone()
            .unwrap_or_else(|| "key".to_string()),
        private_key_path: project.ssh_private_key_path.clone(),
        private_key_passphrase: project.ssh_private_key_passphrase.clone(),
        password: project.ssh_password.clone(),
        deploy_path: Some(project.deploy_path.clone()),
    })
}

pub fn get_project_by_id(conn: &Connection, id: i64) -> Result<Option<Project>> {
    let sql = format!("{PROJECT_SELECT} WHERE p.id = ?1");
    let project = conn.query_row(&sql, params![id], project_from_row).optional()?;
    Ok(project)
}

pub fn list_projects_by_branch(conn: &Connection, branch: &str) -> Result<Vec<Project>> {
    let sql = format!("{PROJECT_SELECT} WHERE p.enabled = 1 AND p.branch = ?1 ORDER BY p.id DESC");
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(params![branch], project_from_row)?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row?);
    }
    Ok(list)
}

/* ------------------------------------------------------------------
   servers
   ------------------------------------------------------------------ */

pub const SERVER_SELECT: &str = "SELECT id, name, group_name, host, port, username, auth_method,
            private_key_path, private_key_passphrase, password, deploy_path, enabled, created_at, updated_at
     FROM servers";

pub fn server_from_row(row: &Row<'_>) -> rusqlite::Result<Server> {
    let private_key_passphrase: Option<String> = row.get(8)?;
    let password: Option<String> = row.get(9)?;

    Ok(Server {
        id: row.get(0)?,
        name: row.get(1)?,
        group_name: row.get(2)?,
        host: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        auth_method: row.get(6)?,
        private_key_path: row.get(7)?,
        private_key_passphrase_set: private_key_passphrase
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        password_set: password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        deploy_path: row.get(10)?,
        enabled: row.get::<_, i64>(11)? == 1,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

pub fn list_servers(conn: &Connection) -> Result<Vec<ServerSummary>> {
    let sql = format!(
        "{SERVER_SELECT} ORDER BY
            CASE WHEN group_name IS NULL OR TRIM(group_name) = '' THEN 1 ELSE 0 END,
            group_name, name, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], server_from_row)?;

    let mut list = Vec::new();
    for row in rows {
        let server = row?;
        let project_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM projects WHERE server_id = ?1",
            params![server.id],
            |r| r.get(0),
        )?;
        list.push(ServerSummary {
            server,
            project_count,
        });
    }
    Ok(list)
}

/// 采样与连接测试需要的完整凭据。绝不序列化给前端。
pub fn deploy_target_for_server(conn: &Connection, id: i64) -> Result<Option<DeployTarget>> {
    let sql = format!("{SERVER_SELECT} WHERE id = ?1");
    let row = conn
        .query_row(&sql, params![id], |row| {
            let server = server_from_row(row)?;
            let passphrase: Option<String> = row.get(8)?;
            let password: Option<String> = row.get(9)?;
            Ok((server, passphrase, password))
        })
        .optional()?;

    Ok(row.map(|(server, passphrase, password)| DeployTarget {
        server_id: server.id,
        name: server.name,
        host: server.host,
        port: server.port,
        user: server.username,
        auth_method: server.auth_method,
        private_key_path: server.private_key_path,
        private_key_passphrase: passphrase,
        password,
        deploy_path: server.deploy_path,
    }))
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

    fn absolute_deploy_path() -> String {
        if cfg!(windows) {
            "D:\\apps\\demo".to_string()
        } else {
            "/srv/apps/demo".to_string()
        }
    }

    fn local_project() -> UpsertProjectRequest {
        UpsertProjectRequest {
            name: "demo".to_string(),
            deploy_mode: "local".to_string(),
            repo_path: "D:\\repos\\demo".to_string(),
            branch: "main".to_string(),
            workdir: None,
            build_command: "npm run build".to_string(),
            built_path: "dist".to_string(),
            server_id: None,
            deploy_path: absolute_deploy_path(),
            stop_command: None,
            start_command: None,
            enabled: true,
        }
    }

    fn key_server() -> UpsertServerRequest {
        UpsertServerRequest {
            name: "prod-web".to_string(),
            group_name: Some("production".to_string()),
            host: "10.0.0.8".to_string(),
            port: 22,
            username: "deploy".to_string(),
            auth_method: "key".to_string(),
            private_key_path: Some("/home/deploy/.ssh/id_ed25519".to_string()),
            private_key_passphrase: None,
            password: None,
            deploy_path: Some("/srv/apps/demo".to_string()),
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

        // ci_ssh 必须挂服务器：没有 server_id 就没有主机与凭据
        project.deploy_mode = "ci_ssh".to_string();
        project.deploy_path = "/srv/apps/demo".to_string();
        assert!(validate_project_payload(&project).is_err());

        project.server_id = Some(1);
        assert!(validate_project_payload(&project).is_ok());
    }

    #[test]
    fn rejects_unsafe_deploy_paths() {
        let mut project = local_project();

        project.deploy_path = "relative/dir".to_string();
        assert!(validate_project_payload(&project).is_err());

        project.deploy_path = if cfg!(windows) {
            "C:\\".to_string()
        } else {
            "/".to_string()
        };
        assert!(validate_project_payload(&project).is_err());

        project.deploy_path = absolute_deploy_path();
        assert!(validate_project_payload(&project).is_ok());

        project.deploy_mode = "ci_ssh".to_string();
        project.server_id = Some(1);

        project.deploy_path = "/".to_string();
        assert!(validate_project_payload(&project).is_err());

        project.deploy_path = "/srv/apps/demo".to_string();
        assert!(validate_project_payload(&project).is_ok());
    }

    #[test]
    fn validates_server_payloads() {
        let server = key_server();
        assert!(validate_server_payload(&server).is_ok());

        let mut blank_name = key_server();
        blank_name.name = "   ".to_string();
        assert!(validate_server_payload(&blank_name).is_err());

        let mut blank_host = key_server();
        blank_host.host = String::new();
        assert!(validate_server_payload(&blank_host).is_err());

        let mut blank_user = key_server();
        blank_user.username = " ".to_string();
        assert!(validate_server_payload(&blank_user).is_err());

        let mut bad_port = key_server();
        bad_port.port = 0;
        assert!(validate_server_payload(&bad_port).is_err());
        bad_port.port = 70_000;
        assert!(validate_server_payload(&bad_port).is_err());
        bad_port.port = 2222;
        assert!(validate_server_payload(&bad_port).is_ok());

        let mut unknown_auth = key_server();
        unknown_auth.auth_method = "agent".to_string();
        assert!(validate_server_payload(&unknown_auth).is_err());
    }

    #[test]
    fn key_auth_requires_a_private_key_path() {
        let mut server = key_server();
        server.private_key_path = None;
        assert!(validate_server_payload(&server).is_err());

        server.private_key_path = Some("   ".to_string());
        assert!(validate_server_payload(&server).is_err());

        server.private_key_path = Some("/home/deploy/.ssh/id_ed25519".to_string());
        assert!(validate_server_payload(&server).is_ok());
    }

    #[test]
    fn password_auth_does_not_require_a_key() {
        let mut server = key_server();
        server.auth_method = "password".to_string();
        // 更新时前端不回传已保存的密码，所以空密码必须放行
        server.private_key_path = None;
        server.password = None;
        assert!(validate_server_payload(&server).is_ok());
    }

    #[test]
    fn server_deploy_path_must_be_absolute() {
        let mut server = key_server();
        server.deploy_path = Some("relative/dir".to_string());
        assert!(validate_server_payload(&server).is_err());

        server.deploy_path = Some("/srv/apps/demo".to_string());
        assert!(validate_server_payload(&server).is_ok());

        // 没填部署目录是允许的，采样时会退回根分区
        server.deploy_path = None;
        assert!(validate_server_payload(&server).is_ok());
    }

    #[test]
    fn migration_pulls_inline_ssh_config_into_servers() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn, Some("strong-password")).unwrap();

        // 模拟旧版本的库：SSH 配置写在 projects 上
        conn.execute(
            "INSERT INTO projects (name, repo_path, branch, build_command, built_path, deploy_path,
                                   enabled, created_at, updated_at, deploy_mode,
                                   ssh_host, ssh_port, ssh_user, ssh_private_key_path)
             VALUES ('demo', 'owner/demo', 'main', 'make', 'dist', '/srv/demo',
                     1, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', 'ci_ssh',
                     '10.0.0.8', 22, 'deploy', '/home/deploy/.ssh/id_ed25519')",
            [],
        )
        .unwrap();

        migrate_ssh_fields_to_servers(&conn).unwrap();

        let (server_id, server_name): (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT p.server_id, s.name FROM projects p LEFT JOIN servers s ON s.id = p.server_id",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(server_id.is_some(), "project was not linked to a server");
        assert_eq!(server_name.as_deref(), Some("demo"));

        // 项目现在能通过 JOIN 拿到完整的 SSH 配置
        let project = get_project_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(project.ssh_host.as_deref(), Some("10.0.0.8"));
        assert_eq!(project.ssh_user.as_deref(), Some("deploy"));
        assert_eq!(project.ssh_port, Some(22));
        assert_eq!(project.ssh_auth_method.as_deref(), Some("key"));
        assert!(deploy_target_for_project(&project).is_some());
    }

    #[test]
    fn migration_is_idempotent_and_reuses_shared_hosts() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn, Some("strong-password")).unwrap();

        for name in ["a", "b"] {
            conn.execute(
                "INSERT INTO projects (name, repo_path, branch, build_command, built_path, deploy_path,
                                       enabled, created_at, updated_at, deploy_mode,
                                       ssh_host, ssh_port, ssh_user, ssh_private_key_path)
                 VALUES (?1, 'owner/x', 'main', 'make', 'dist', '/srv/x',
                         1, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', 'ci_ssh',
                         '10.0.0.8', 22, 'deploy', '/k')",
                params![name],
            )
            .unwrap();
        }

        migrate_ssh_fields_to_servers(&conn).unwrap();
        let after_first: i64 = conn
            .query_row("SELECT COUNT(*) FROM servers", [], |row| row.get(0))
            .unwrap();
        // 同一台机器被两个项目引用时只建一条服务器记录
        assert_eq!(after_first, 1, "same host should share one server row");

        // 再跑几次不能新增
        migrate_ssh_fields_to_servers(&conn).unwrap();
        migrate_ssh_fields_to_servers(&conn).unwrap();
        let after_repeat: i64 = conn
            .query_row("SELECT COUNT(*) FROM servers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after_repeat, 1, "migration is not idempotent");

        let linked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE server_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked, 2);
    }

    #[test]
    fn local_projects_are_left_alone_by_migration() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn, Some("strong-password")).unwrap();
        conn.execute(
            "INSERT INTO projects (name, repo_path, branch, build_command, built_path, deploy_path,
                                   enabled, created_at, updated_at, deploy_mode)
             VALUES ('local-only', 'D:\\repos\\x', 'main', 'make', 'dist', '/srv/x',
                     1, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', 'local')",
            [],
        )
        .unwrap();

        migrate_ssh_fields_to_servers(&conn).unwrap();

        let servers: i64 = conn
            .query_row("SELECT COUNT(*) FROM servers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(servers, 0, "local project should not create a server");

        let project = get_project_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(project.server_id, None);
        assert!(deploy_target_for_project(&project).is_none());
    }

    #[test]
    fn marks_interrupted_deployments_as_failed() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn, Some("strong-password")).unwrap();
        conn.execute(
            "INSERT INTO projects (name, repo_path, branch, build_command, built_path, deploy_path, enabled, created_at, updated_at)
             VALUES ('demo', 'owner/demo', 'main', 'make', 'dist', '/srv/demo', 1, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')",
            [],
        )
        .unwrap();
        let project_id = conn.last_insert_rowid();

        for status in ["running", "queued", "success"] {
            conn.execute(
                "INSERT INTO deployments (project_id, branch, status, message, started_at)
                 VALUES (?1, 'main', ?2, 'note', '2026-01-01T00:00:00+00:00')",
                params![project_id, status],
            )
            .unwrap();
        }

        assert_eq!(fail_interrupted_deployments(&conn).unwrap(), 2);

        let rows = conn
            .prepare("SELECT status, message, finished_at FROM deployments ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .unwrap()
            .map(|row| row.unwrap())
            .collect::<Vec<_>>();

        assert_eq!(rows[0].0, "failed");
        assert!(rows[0].1.as_deref().unwrap().contains("note"));
        assert!(rows[0].2.is_some());
        assert_eq!(rows[1].0, "failed");
        assert_eq!(rows[2].0, "success");
        assert!(rows[2].2.is_none());

        // Nothing is left in an in-progress state on a later boot either.
        assert_eq!(fail_interrupted_deployments(&conn).unwrap(), 0);
    }
}
