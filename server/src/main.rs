use anyhow::{anyhow, bail, Context, Result};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::Sha256;
use std::{
    env, fs,
    path::{Path as FsPath, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use subtle::ConstantTimeEq;
use tokio::task;
use tower_http::cors::{Any, CorsLayer};

mod auth;
mod db;
mod models;
mod state;
mod utils;

use auth::{authenticate, hash_password, issue_token, verify_password};
use db::{
    get_project_by_id, get_setting, init_db, insert_deployment, list_projects_by_branch,
    update_deployment_status, validate_project_payload,
};
use models::{
    ApiUser, ChangePasswordRequest, Deployment, DeploymentDetail, ListDeploymentsQuery,
    LoginRequest, LoginResponse, MessageResponse, Project, PushPayload, UpdateWebhookSecretRequest,
    UpsertProjectRequest, WebhookSecretResponse,
};
use state::AppState;
use utils::{json_error, log, now_ts};

#[tokio::main]
async fn main() -> Result<()> {
    let db_path = env::var("AUTO_DEPLOY_DB").unwrap_or_else(|_| "auto_deploy.db".to_string());
    let jwt_secret =
        env::var("JWT_SECRET").unwrap_or_else(|_| "change-me-in-production".to_string());
    let git_path = env::var("GIT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("git"));

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open sqlite db: {db_path}"))?;
    init_db(&conn)?;

    let bind = get_setting(&conn, "webhook_bind")?.unwrap_or_else(|| "0.0.0.0:8000".to_string());
    let webhook_path =
        get_setting(&conn, "webhook_path")?.unwrap_or_else(|| "/webhook".to_string());

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        deploy_lock: Arc::new(Mutex::new(())),
        jwt_secret: Arc::new(jwt_secret),
        git_path: Arc::new(git_path),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    let app = Router::new()
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/me", get(auth_me))
        .route("/api/auth/change-password", post(change_password))
        .route(
            "/api/settings/webhook-secret",
            get(get_webhook_secret).put(update_webhook_secret),
        )
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/:id",
            put(update_project).delete(delete_project),
        )
        .route("/api/deploy/:project_id", post(trigger_deploy))
        .route("/api/deployments", get(list_deployments))
        .route("/api/deployments/:id", get(get_deployment_detail))
        .route(&webhook_path, post(handle_webhook))
        .layer(cors)
        .with_state(state);

    log(&format!("SQLite initialized at {db_path}"));
    log(&format!("Server listening at http://{bind}"));
    log(&format!("Webhook endpoint: {webhook_path}"));

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("Failed to bind {bind}"))?;
    axum::serve(listener, app)
        .await
        .context("Server exited unexpectedly")?;

    Ok(())
}

async fn auth_login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> Response {
    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    let user_row = match conn
        .query_row(
            "SELECT id, username, password_hash FROM users WHERE username = ?1",
            params![payload.username],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
    {
        Ok(v) => v,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    let Some((id, username, password_hash)) = user_row else {
        return json_error(StatusCode::UNAUTHORIZED, "用户名或密码错误").into_response();
    };

    match verify_password(&payload.password, &password_hash) {
        Ok(true) => {}
        Ok(false) => {
            return json_error(StatusCode::UNAUTHORIZED, "用户名或密码错误").into_response()
        }
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }

    let token = match issue_token(&state, &username) {
        Ok(t) => t,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    (
        StatusCode::OK,
        Json(LoginResponse {
            token,
            user: ApiUser { id, username },
        }),
    )
        .into_response()
}

async fn auth_me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match authenticate(&headers, &state) {
        Ok(user) => (StatusCode::OK, Json(user)).into_response(),
        Err(err) => json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response(),
    }
}

async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    if payload.new_password.len() < 6 {
        return json_error(StatusCode::BAD_REQUEST, "新密码至少 6 位").into_response();
    }

    let user = match authenticate(&headers, &state) {
        Ok(u) => u,
        Err(err) => return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response(),
    };

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    let current_hash: String = match conn.query_row(
        "SELECT password_hash FROM users WHERE id = ?1",
        params![user.id],
        |row| row.get(0),
    ) {
        Ok(v) => v,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    match verify_password(&payload.old_password, &current_hash) {
        Ok(true) => {}
        Ok(false) => return json_error(StatusCode::BAD_REQUEST, "旧密码不正确").into_response(),
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }

    let new_hash = match hash_password(&payload.new_password) {
        Ok(v) => v,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    if let Err(err) = conn.execute(
        "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_hash, now_ts(), user.id],
    ) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    (
        StatusCode::OK,
        Json(MessageResponse {
            message: "修改成功".to_string(),
        }),
    )
        .into_response()
}

async fn get_webhook_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    let secret = match get_setting(&conn, "webhook_secret") {
        Ok(v) => v.unwrap_or_default(),
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    (
        StatusCode::OK,
        Json(WebhookSecretResponse {
            webhook_secret: secret,
        }),
    )
        .into_response()
}

async fn update_webhook_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateWebhookSecretRequest>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    if let Err(err) = conn.execute(
        "INSERT INTO settings (key, value) VALUES ('webhook_secret', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![payload.webhook_secret.trim()],
    ) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    (
        StatusCode::OK,
        Json(MessageResponse {
            message: "webhook_secret 已更新".to_string(),
        }),
    )
        .into_response()
}

async fn list_projects(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response();
        }
    };

    let mut stmt = match conn.prepare(
        "SELECT id, name, repo_path, branch, workdir, build_command, built_path, deploy_path, stop_command, start_command, enabled, created_at, updated_at
         FROM projects ORDER BY id DESC",
    ) {
        Ok(s) => s,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    let rows = match stmt.query_map([], |row| {
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
    }) {
        Ok(r) => r,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    let mut list = Vec::new();
    for item in rows {
        match item {
            Ok(p) => list.push(p),
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        }
    }

    (StatusCode::OK, Json(list)).into_response()
}

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpsertProjectRequest>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    if let Err(err) = validate_project_payload(&payload) {
        return json_error(StatusCode::BAD_REQUEST, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response();
        }
    };

    let now = now_ts();
    let enabled = if payload.enabled { 1 } else { 0 };
    let result = conn.execute(
        "INSERT INTO projects (name, repo_path, branch, workdir, build_command, built_path, deploy_path, stop_command, start_command, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            payload.name,
            payload.repo_path,
            payload.branch,
            payload.workdir,
            payload.build_command,
            payload.built_path,
            payload.deploy_path,
            payload.stop_command,
            payload.start_command,
            enabled,
            now,
        ],
    );

    match result {
        Ok(_) => (
            StatusCode::CREATED,
            Json(MessageResponse {
                message: "项目已创建".to_string(),
            }),
        )
            .into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}
async fn update_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(payload): Json<UpsertProjectRequest>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    if let Err(err) = validate_project_payload(&payload) {
        return json_error(StatusCode::BAD_REQUEST, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response();
        }
    };

    let enabled = if payload.enabled { 1 } else { 0 };
    let result = conn.execute(
        "UPDATE projects
         SET name = ?1, repo_path = ?2, branch = ?3, workdir = ?4, build_command = ?5, built_path = ?6, deploy_path = ?7,
             stop_command = ?8, start_command = ?9, enabled = ?10, updated_at = ?11
         WHERE id = ?12",
        params![
            payload.name,
            payload.repo_path,
            payload.branch,
            payload.workdir,
            payload.build_command,
            payload.built_path,
            payload.deploy_path,
            payload.stop_command,
            payload.start_command,
            enabled,
            now_ts(),
            id,
        ],
    );

    match result {
        Ok(affected) if affected > 0 => (
            StatusCode::OK,
            Json(MessageResponse {
                message: "项目已更新".to_string(),
            }),
        )
            .into_response(),
        Ok(_) => json_error(StatusCode::NOT_FOUND, "项目不存在").into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    if let Err(err) = conn.execute("DELETE FROM deployments WHERE project_id = ?1", params![id]) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    match conn.execute("DELETE FROM projects WHERE id = ?1", params![id]) {
        Ok(affected) if affected > 0 => (
            StatusCode::OK,
            Json(MessageResponse {
                message: "项目已删除".to_string(),
            }),
        )
            .into_response(),
        Ok(_) => json_error(StatusCode::NOT_FOUND, "项目不存在").into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn trigger_deploy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let (project, deploy_id) = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(_) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                    .into_response();
            }
        };

        let project = match get_project_by_id(&conn, project_id) {
            Ok(Some(p)) => p,
            Ok(None) => return json_error(StatusCode::NOT_FOUND, "项目不存在").into_response(),
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        };

        let deploy_id = match insert_deployment(&conn, project.id, &project.branch, "queued", None)
        {
            Ok(id) => id,
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        };

        (project, deploy_id)
    };

    let state_clone = state.clone();
    task::spawn_blocking(move || {
        run_project_deployment(state_clone, project, deploy_id);
    });

    (
        StatusCode::ACCEPTED,
        Json(MessageResponse {
            message: format!("部署任务已创建: {}", deploy_id),
        }),
    )
        .into_response()
}

async fn list_deployments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListDeploymentsQuery>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response();
        }
    };

    let mut stmt = match conn.prepare(
        "SELECT d.id, d.project_id, p.name, d.branch, d.status, d.message, d.started_at, d.finished_at
         FROM deployments d
         JOIN projects p ON p.id = d.project_id
         ORDER BY d.id DESC
         LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    let rows = match stmt.query_map(params![limit as i64], |row| {
        Ok(Deployment {
            id: row.get(0)?,
            project_id: row.get(1)?,
            project_name: row.get(2)?,
            branch: row.get(3)?,
            status: row.get(4)?,
            message: row.get(5)?,
            started_at: row.get(6)?,
            finished_at: row.get(7)?,
        })
    }) {
        Ok(r) => r,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    let mut list = Vec::new();
    for item in rows {
        match item {
            Ok(v) => list.push(v),
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        }
    }

    (StatusCode::OK, Json(list)).into_response()
}

async fn get_deployment_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    let detail = match conn
        .query_row(
            "SELECT d.id, d.project_id, p.name, d.branch, d.status, d.message, d.started_at, d.finished_at, d.log_text
             FROM deployments d
             JOIN projects p ON p.id = d.project_id
             WHERE d.id = ?1",
            params![id],
            |row| {
                Ok(DeploymentDetail {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    project_name: row.get(2)?,
                    branch: row.get(3)?,
                    status: row.get(4)?,
                    message: row.get(5)?,
                    started_at: row.get(6)?,
                    finished_at: row.get(7)?,
                    log_text: row.get(8)?,
                })
            },
        )
        .optional()
    {
        Ok(v) => v,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    match detail {
        Some(v) => (StatusCode::OK, Json(v)).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "部署记录不存在").into_response(),
    }
}

async fn handle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(err) = verify_github_signature(&state, &headers, &body) {
        log(&format!("Webhook signature failed: {err:#}"));
        return StatusCode::UNAUTHORIZED;
    }

    let event = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if event != "push" {
        return StatusCode::NO_CONTENT;
    }

    let payload: PushPayload = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            log(&format!("Webhook payload parse error: {err}"));
            return StatusCode::BAD_REQUEST;
        }
    };

    let Some(branch) = extract_branch(&payload.git_ref) else {
        return StatusCode::BAD_REQUEST;
    };

    let projects = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
        };

        match list_projects_by_branch(&conn, &branch) {
            Ok(v) => v,
            Err(err) => {
                log(&format!("Query project failed: {err:#}"));
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        }
    };

    if projects.is_empty() {
        log(&format!("No enabled project for branch: {branch}"));
        return StatusCode::ACCEPTED;
    }

    let state_clone = state.clone();
    task::spawn_blocking(move || {
        for project in projects {
            let deploy_id = {
                let conn = match state_clone.db.lock() {
                    Ok(c) => c,
                    Err(_) => {
                        log("DB lock poisoned when inserting deployment");
                        continue;
                    }
                };
                match insert_deployment(
                    &conn,
                    project.id,
                    &branch,
                    "queued",
                    Some("Triggered by webhook"),
                ) {
                    Ok(id) => id,
                    Err(err) => {
                        log(&format!("Insert deployment failed: {err:#}"));
                        continue;
                    }
                }
            };

            run_project_deployment(state_clone.clone(), project, deploy_id);
        }
    });

    StatusCode::ACCEPTED
}

fn run_project_deployment(state: AppState, project: Project, deploy_id: i64) {
    let _deploy_guard = match state.deploy_lock.lock() {
        Ok(g) => g,
        Err(_) => {
            let _ = update_deployment_status(
                &state,
                deploy_id,
                "failed",
                Some("Deploy lock poisoned".to_string()),
                Some("deploy lock poisoned".to_string()),
            );
            return;
        }
    };

    let _ = update_deployment_status(
        &state,
        deploy_id,
        "running",
        Some("部署进行中".to_string()),
        Some(String::new()),
    );

    let mut deploy_log = String::new();
    let run_result = check_and_deploy_project(&state, &project, &project.branch, &mut deploy_log);

    match run_result {
        Ok(updated) => {
            let msg = if updated {
                format!("Deployment succeeded: {}", project.name)
            } else {
                format!("No update found: {}", project.name)
            };
            log(&msg);
            let _ =
                update_deployment_status(&state, deploy_id, "success", Some(msg), Some(deploy_log));
        }
        Err(err) => {
            let msg = format!("Deployment failed: {} - {err:#}", project.name);
            log(&msg);
            let _ =
                update_deployment_status(&state, deploy_id, "failed", Some(msg), Some(deploy_log));
        }
    }
}

fn check_and_deploy_project(
    state: &AppState,
    project: &Project,
    branch: &str,
    deploy_log: &mut String,
) -> Result<bool> {
    let repo_path = FsPath::new(&project.repo_path);

    log_line(deploy_log, &format!("Start project: {}", project.name));
    git_fetch(&state.git_path, repo_path, branch, deploy_log)?;

    let local = git_rev(&state.git_path, repo_path, "HEAD", deploy_log)?;
    let remote = git_rev(
        &state.git_path,
        repo_path,
        &format!("origin/{branch}"),
        deploy_log,
    )?;
    log_line(deploy_log, &format!("Local: {local}, Remote: {remote}"));

    if local == remote {
        log_line(deploy_log, "No updates detected.");
        return Ok(false);
    }

    git_pull(&state.git_path, repo_path, branch, deploy_log)?;

    let workdir = project
        .workdir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_path.to_path_buf());

    run_shell(&project.build_command, Some(&workdir), deploy_log)?;

    if let Some(cmd) = &project.stop_command {
        run_shell(cmd, None, deploy_log)?;
    }

    deploy_artifact(
        &PathBuf::from(&project.built_path),
        &PathBuf::from(&project.deploy_path),
    )?;
    log_line(
        deploy_log,
        &format!(
            "Artifact copied: {} -> {}",
            project.built_path, project.deploy_path
        ),
    );

    if let Some(cmd) = &project.start_command {
        run_shell(cmd, None, deploy_log)?;
    }

    Ok(true)
}

fn git_fetch(
    git_path: &FsPath,
    repo: &FsPath,
    branch: &str,
    deploy_log: &mut String,
) -> Result<()> {
    run_cmd(
        Command::new(git_path)
            .arg("-C")
            .arg(repo)
            .arg("fetch")
            .arg("origin")
            .arg(branch),
        deploy_log,
    )
    .context("git fetch failed")
}

fn git_pull(git_path: &FsPath, repo: &FsPath, branch: &str, deploy_log: &mut String) -> Result<()> {
    run_cmd(
        Command::new(git_path)
            .arg("-C")
            .arg(repo)
            .arg("pull")
            .arg("--ff-only")
            .arg("origin")
            .arg(branch),
        deploy_log,
    )
    .context("git pull failed")
}

fn git_rev(git_path: &FsPath, repo: &FsPath, rev: &str, deploy_log: &mut String) -> Result<String> {
    let output = Command::new(git_path)
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg(rev)
        .output()
        .with_context(|| format!("git rev-parse {rev} failed"))?;
    log_line(
        deploy_log,
        &format!(
            "$ {} -C {} rev-parse {rev}",
            git_path.display(),
            repo.display()
        ),
    );
    if !output.stdout.is_empty() {
        log_line(
            deploy_log,
            &format!("stdout: {}", String::from_utf8_lossy(&output.stdout).trim()),
        );
    }
    if !output.stderr.is_empty() {
        log_line(
            deploy_log,
            &format!("stderr: {}", String::from_utf8_lossy(&output.stderr).trim()),
        );
    }

    if !output.status.success() {
        bail!(
            "git rev-parse {rev} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn deploy_artifact(src: &FsPath, dst: &FsPath) -> Result<()> {
    let src_meta =
        fs::metadata(src).with_context(|| format!("Source path not found: {}", src.display()))?;

    if src_meta.is_dir() {
        if dst.exists() && !dst.is_dir() {
            bail!(
                "Deploy target must be directory for dir source: {}",
                dst.display()
            );
        }
        if dst.exists() {
            fs::remove_dir_all(dst)
                .with_context(|| format!("Failed to remove old deploy dir: {}", dst.display()))?;
        }
        fs::create_dir_all(dst)
            .with_context(|| format!("Failed to create deploy dir: {}", dst.display()))?;
        copy_dir_all(src, dst)?;
        return Ok(());
    }

    let dst_is_dir = dst.exists() && dst.is_dir();
    let final_dst = if dst_is_dir {
        let file_name = src
            .file_name()
            .ok_or_else(|| anyhow!("Source file has no file name: {}", src.display()))?;
        dst.join(file_name)
    } else {
        dst.to_path_buf()
    };

    if let Some(parent) = final_dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create deploy parent dir: {}", parent.display()))?;
    }

    fs::copy(src, &final_dst).with_context(|| {
        format!(
            "Failed to copy from {} to {}",
            src.display(),
            final_dst.display()
        )
    })?;

    Ok(())
}

fn copy_dir_all(src: &FsPath, dst: &FsPath) -> Result<()> {
    for entry in fs::read_dir(src).with_context(|| format!("Read dir failed: {}", src.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&to)
                .with_context(|| format!("Create dir failed: {}", to.display()))?;
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to).with_context(|| {
                format!("Copy failed from {} to {}", from.display(), to.display())
            })?;
        }
    }
    Ok(())
}

fn run_shell(cmd: &str, workdir: Option<&FsPath>, deploy_log: &mut String) -> Result<()> {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };

    if let Some(dir) = workdir {
        command.current_dir(dir);
    }

    run_cmd(&mut command, deploy_log)
}

fn run_cmd(cmd: &mut Command, deploy_log: &mut String) -> Result<()> {
    log_line(deploy_log, &format!("$ {}", format_command(cmd)));
    let output = cmd.output().context("Failed to spawn command")?;
    if !output.stdout.is_empty() {
        log_line(
            deploy_log,
            &format!(
                "stdout:\n{}",
                String::from_utf8_lossy(&output.stdout).trim()
            ),
        );
    }
    if !output.stderr.is_empty() {
        log_line(
            deploy_log,
            &format!(
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        );
    }
    if !output.status.success() {
        bail!(
            "Command failed: {}\nSTDOUT: {}\nSTDERR: {}",
            format_command(cmd),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn log_line(deploy_log: &mut String, line: &str) {
    deploy_log.push_str(line);
    deploy_log.push('\n');
}

fn format_command(cmd: &Command) -> String {
    let mut text = cmd.get_program().to_string_lossy().to_string();
    for arg in cmd.get_args() {
        text.push(' ');
        text.push_str(&arg.to_string_lossy());
    }
    text
}

fn extract_branch(git_ref: &str) -> Option<String> {
    git_ref.strip_prefix("refs/heads/").map(|v| v.to_string())
}

fn verify_github_signature(state: &AppState, headers: &HeaderMap, body: &[u8]) -> Result<()> {
    let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
    let secret = get_setting(&conn, "webhook_secret")?.unwrap_or_default();
    if secret.is_empty() {
        return Ok(());
    }

    let sig = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .context("Missing X-Hub-Signature-256")?;

    let sig = sig
        .strip_prefix("sha256=")
        .context("Invalid signature prefix")?;
    let sig_bytes = hex::decode(sig).context("Invalid signature hex")?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).context("Invalid secret")?;
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.ct_eq(&sig_bytes).unwrap_u8() != 1 {
        bail!("Signature mismatch");
    }

    Ok(())
}
