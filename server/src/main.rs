use anyhow::{anyhow, bail, Context, Result};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderMap, HeaderName, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::Sha256;
use std::{
    env, fs,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{sync::Semaphore, task};
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

mod auth;
mod db;
mod deploy;
mod logging;
mod metrics;
mod models;
mod net;
mod state;
mod utils;

use auth::{authenticate, hash_password, issue_token, verify_password, LoginThrottle};
use db::{
    deploy_target_for_server, fail_interrupted_deployments, get_or_create_jwt_secret,
    get_project_by_id, get_setting, init_db, insert_deployment, list_projects_by_branch,
    list_servers, project_from_row, update_deployment_status, validate_project_payload,
    validate_server_payload, PROJECT_SELECT,
};
use models::{
    ApiUser, ArtifactReadyRequest, ChangePasswordRequest, Deployment, DeploymentDetail,
    ListDeploymentsQuery, LoginRequest, LoginResponse, MessageResponse, Project, PushPayload,
    ReleaseAsset, ReleaseWebhookPayload, ServerMetrics, ServerMetricsResponse, ServerTestResponse,
    UpdateWebhookSecretRequest, UpsertProjectRequest, UpsertServerRequest, WebhookSecretResponse,
};
use state::AppState;
use utils::{debug, error, json_error, log, now_ts, warn};

/// 可选字段的空串归一化成 NULL：前端表单清空字段时传的是 ""，
/// 直接落库会让 `TRIM(x) = ''` 和 NULL 两种「空」在判断里分叉。
fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 前端构建产物目录。
///
/// 优先读 `KEEL_WEB_ROOT`（容器里指向 /app/web），否则回退到相对可执行文件
/// 的几个常见位置，方便直接在仓库里 `cargo run`。
fn web_root() -> Option<PathBuf> {
    if let Ok(value) = env::var("KEEL_WEB_ROOT") {
        let path = PathBuf::from(value);
        if path.join("index.html").is_file() {
            return Some(path);
        }
        warn(&format!(
            "KEEL_WEB_ROOT 指向的目录没有 index.html: {}",
            path.display()
        ));
    }

    let candidates = ["web/dist", "../web/dist", "../../web/dist"];
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    logging::init()?;

    let db_path = env::var("AUTO_DEPLOY_DB").unwrap_or_else(|_| "auto_deploy.db".to_string());
    let git_path = env::var("GIT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("git"));
    let workspace_root = env::var("AUTO_DEPLOY_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("workspaces"));
    fs::create_dir_all(&workspace_root)
        .with_context(|| format!("Failed to create workspace: {}", workspace_root.display()))?;
    let workspace_display = workspace_root.display().to_string();

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open sqlite db: {db_path}"))?;
    let bootstrap_password = env::var("ADMIN_PASSWORD").ok();
    let init_result = init_db(&conn, bootstrap_password.as_deref())?;
    if let Some(password) = init_result.generated_admin_password {
        log("A secure admin password was generated. Save it now and change it after login.");
        log(&format!("Initial admin credentials: admin / {password}"));
    }
    let interrupted = fail_interrupted_deployments(&conn)?;
    if interrupted > 0 {
        warn(&format!(
            "Marked {interrupted} deployment(s) interrupted by a restart as failed."
        ));
    }

    let jwt_secret = match env::var("JWT_SECRET") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            log("JWT_SECRET is not set; using a persistent secret stored in SQLite.");
            get_or_create_jwt_secret(&conn)?
        }
    };
    let trust_proxy = env::var("TRUST_PROXY").unwrap_or_default();
    let trusted_proxies = net::TrustedProxies::parse(&trust_proxy)
        .with_context(|| format!("Failed to parse TRUST_PROXY: {}", trust_proxy.trim()))?;

    let bind = get_setting(&conn, "webhook_bind")?.unwrap_or_else(|| "0.0.0.0:8000".to_string());
    let webhook_path =
        get_setting(&conn, "webhook_path")?.unwrap_or_else(|| "/webhook".to_string());
    let webhook_secret = get_setting(&conn, "webhook_secret")?.unwrap_or_default();
    if webhook_secret.trim().is_empty() && !bind_is_loopback(&bind) {
        warn(&format!(
            "webhook_secret 为空且监听在 {bind}，Push/Release webhook 不校验签名；\
             任何能访问该地址的人都可以触发部署，请在系统设置中配置 webhook_secret。"
        ));
    }
    if trusted_proxies.is_enabled() {
        log(&format!("Trusted proxy peers: {}", trust_proxy.trim()));
    }
    let deploy_concurrency = env::var("DEPLOY_MAX_CONCURRENCY")
        .ok()
        .as_deref()
        .map(|value| parse_deploy_concurrency(Some(value)))
        .unwrap_or_else(|| parse_deploy_concurrency(None));

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        deploy_semaphore: Arc::new(Semaphore::new(deploy_concurrency)),
        project_locks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        jwt_secret: Arc::new(jwt_secret),
        trusted_proxies: Arc::new(trusted_proxies),
        login_throttle: Arc::new(LoginThrottle::new()),
        git_path: Arc::new(git_path),
        workspace_root: Arc::new(workspace_root),
    };
    let shutdown_state = state.clone();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-auto-deploy-signature"),
        ]);

    let app = Router::new()
        .route("/health", get(health))
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
        .route("/api/deploy/callback", post(handle_artifact_callback))
        .route("/api/deploy/:project_id", post(trigger_deploy))
        .route("/api/deployments", get(list_deployments))
        .route("/api/deployments/:id", get(get_deployment_detail))
        .route("/api/servers", get(list_servers_handler).post(create_server))
        .route(
            "/api/servers/:id",
            put(update_server).delete(delete_server),
        )
        .route("/api/servers/:id/test", post(test_server_connection))
        .route("/api/servers/:id/metrics", get(server_metrics))
        .route("/api/metrics", get(all_metrics))
        .route(&webhook_path, post(handle_webhook));

    // 前端构建产物。目录不存在时（本地前后端分离开发）直接跳过，
    // 由 vite dev server 提供页面。
    let app = match web_root() {
        Some(dir) => {
            log(&format!("Serving web UI from {}", dir.display()));
            let index = dir.join("index.html");
            app.fallback_service(
                ServeDir::new(&dir).not_found_service(ServeFile::new(index)),
            )
        }
        None => {
            log("No web build found (set KEEL_WEB_ROOT to serve the UI)");
            app
        }
    };

    let app = app
        .layer(cors)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state);

    log(&format!("SQLite initialized at {db_path}"));
    log(&format!("Server listening at http://{bind}"));
    log(&format!("Webhook endpoint: {webhook_path}"));
    log(&format!(
        "Deployment concurrency limit: {deploy_concurrency}"
    ));
    log(&format!("Managed workspace: {}", workspace_display));

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("Failed to bind {bind}"))?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("Server exited unexpectedly")?;

    drain_running_deployments(&shutdown_state, deploy_concurrency).await;

    Ok(())
}

async fn auth_login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let login_ip = net::client_ip(&headers, peer, &state.trusted_proxies);
    let throttle_key = login_ip.to_string();

    if let Some(retry_after) = state.login_throttle.retry_after(&throttle_key) {
        warn(&format!("Login throttled for {throttle_key}"));
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "登录尝试过于频繁，请在 {} 秒后重试",
                retry_after.as_secs().max(1)
            ),
        )
        .into_response();
    }

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
        state.login_throttle.record_failure(&throttle_key);
        return json_error(StatusCode::UNAUTHORIZED, "用户名或密码错误").into_response();
    };

    match verify_password(&payload.password, &password_hash) {
        Ok(true) => state.login_throttle.record_success(&throttle_key),
        Ok(false) => {
            state.login_throttle.record_failure(&throttle_key);
            return json_error(StatusCode::UNAUTHORIZED, "用户名或密码错误").into_response();
        }
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }

    let login_ip_text = login_ip.to_string();
    let login_at = now_ts();
    if let Err(err) = conn.execute(
        "UPDATE users
         SET last_login_ip = ?1, last_login_at = ?2
         WHERE id = ?3",
        params![login_ip_text, login_at, id],
    ) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
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
            user: ApiUser {
                id,
                username,
                last_login_ip: Some(login_ip_text),
                last_login_at: Some(login_at),
            },
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

/// Unauthenticated liveness probe for systemd, containers and load balancers.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let database_reachable = state
        .db
        .lock()
        .ok()
        .and_then(|conn| {
            conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .ok()
        })
        .is_some();

    if database_reachable {
        (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "unavailable" })),
        )
            .into_response()
    }
}

async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    if payload.new_password.len() < 8 {
        return json_error(StatusCode::BAD_REQUEST, "新密码至少 8 位").into_response();
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

    let mut stmt = match conn.prepare(&format!("{PROJECT_SELECT} ORDER BY p.id DESC")) {
        Ok(s) => s,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    let rows = match stmt.query_map([], project_from_row) {
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
        "INSERT INTO projects (name, repo_path, branch, workdir, build_command, built_path, artifact_url, artifact_sha256, server_id, deploy_path, stop_command, start_command, enabled, created_at, updated_at, deploy_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?15)",
        params![
            payload.name,
            payload.repo_path,
            payload.branch,
            payload.workdir,
            payload.build_command,
            payload.built_path,
            Option::<String>::None,
            Option::<String>::None,
            payload.server_id,
            payload.deploy_path,
            payload.stop_command,
            payload.start_command,
            enabled,
            now,
            payload.deploy_mode,
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
         SET name = ?1, repo_path = ?2, branch = ?3, workdir = ?4, build_command = ?5, built_path = ?6,
             artifact_url = ?7, artifact_sha256 = ?8, server_id = ?9,
             deploy_path = ?10, stop_command = ?11, start_command = ?12, enabled = ?13,
             updated_at = ?14, deploy_mode = ?15
         WHERE id = ?16",
        params![
            payload.name,
            payload.repo_path,
            payload.branch,
            payload.workdir,
            payload.build_command,
            payload.built_path,
            Option::<String>::None,
            Option::<String>::None,
            payload.server_id,
            payload.deploy_path,
            payload.stop_command,
            payload.start_command,
            enabled,
            now_ts(),
            payload.deploy_mode,
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

/* ------------------------------------------------------------------
   servers
   ------------------------------------------------------------------ */

async fn list_servers_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
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

    match list_servers(&conn) {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn create_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpsertServerRequest>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }
    if let Err(err) = validate_server_payload(&payload) {
        return json_error(StatusCode::BAD_REQUEST, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    let now = now_ts();
    let result = conn.execute(
        "INSERT INTO servers
         (name, group_name, host, port, username, auth_method, private_key_path,
          private_key_passphrase, password, deploy_path, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![
            payload.name.trim(),
            trimmed(payload.group_name.as_deref()),
            payload.host.trim(),
            payload.port,
            payload.username.trim(),
            payload.auth_method,
            trimmed(payload.private_key_path.as_deref()),
            trimmed(payload.private_key_passphrase.as_deref()),
            trimmed(payload.password.as_deref()),
            trimmed(payload.deploy_path.as_deref()),
            i64::from(payload.enabled),
            now,
        ],
    );

    match result {
        Ok(_) => (
            StatusCode::CREATED,
            Json(MessageResponse {
                message: "服务器已创建".to_string(),
            }),
        )
            .into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn update_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(payload): Json<UpsertServerRequest>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }
    if let Err(err) = validate_server_payload(&payload) {
        return json_error(StatusCode::BAD_REQUEST, err.to_string()).into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                .into_response()
        }
    };

    // 凭据字段是「三态」：None 保持原值、空串清除、有值覆盖。
    // 前端不会回传已保存的密码，所以不能把 None 当成清空。
    let result = conn.execute(
        "UPDATE servers
         SET name = ?1, group_name = ?2, host = ?3, port = ?4, username = ?5, auth_method = ?6,
             private_key_path = ?7,
             private_key_passphrase = CASE
                 WHEN ?8 IS NULL THEN private_key_passphrase
                 WHEN TRIM(?8) = '' THEN NULL
                 ELSE ?8
             END,
             password = CASE
                 WHEN ?9 IS NULL THEN password
                 WHEN TRIM(?9) = '' THEN NULL
                 ELSE ?9
             END,
             deploy_path = ?10, enabled = ?11, updated_at = ?12
         WHERE id = ?13",
        params![
            payload.name.trim(),
            trimmed(payload.group_name.as_deref()),
            payload.host.trim(),
            payload.port,
            payload.username.trim(),
            payload.auth_method,
            trimmed(payload.private_key_path.as_deref()),
            payload.private_key_passphrase,
            payload.password,
            trimmed(payload.deploy_path.as_deref()),
            i64::from(payload.enabled),
            now_ts(),
            id,
        ],
    );

    match result {
        Ok(affected) if affected > 0 => (
            StatusCode::OK,
            Json(MessageResponse {
                message: "服务器已更新".to_string(),
            }),
        )
            .into_response(),
        Ok(_) => json_error(StatusCode::NOT_FOUND, "服务器不存在").into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn delete_server(
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

    let referenced: i64 = match conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE server_id = ?1",
        params![id],
        |row| row.get(0),
    ) {
        Ok(count) => count,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    };

    // 直接删会让引用它的项目失去部署目标，而报错信息里根本看不出是哪台服务器，
    // 所以这里明确拒绝，让用户先去改项目。
    if referenced > 0 {
        return json_error(
            StatusCode::CONFLICT,
            format!("仍有 {referenced} 个项目在使用该服务器，请先改为其他服务器"),
        )
        .into_response();
    }

    match conn.execute("DELETE FROM servers WHERE id = ?1", params![id]) {
        Ok(affected) if affected > 0 => (
            StatusCode::OK,
            Json(MessageResponse {
                message: "服务器已删除".to_string(),
            }),
        )
            .into_response(),
        Ok(_) => json_error(StatusCode::NOT_FOUND, "服务器不存在").into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn test_server_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let target = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(_) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                    .into_response()
            }
        };
        match deploy_target_for_server(&conn, id) {
            Ok(Some(target)) => target,
            Ok(None) => return json_error(StatusCode::NOT_FOUND, "服务器不存在").into_response(),
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        }
    };

    // SSH 握手是阻塞的，放到 blocking 线程池，避免占住 async worker。
    let outcome = task::spawn_blocking(move || {
        let mut log = String::new();
        let result = deploy::test_connection(&target, &mut log);
        (result, log)
    })
    .await;

    match outcome {
        Ok((Ok(()), log)) => (
            StatusCode::OK,
            Json(ServerTestResponse {
                ok: true,
                message: "连接正常".to_string(),
                detail: log,
            }),
        )
            .into_response(),
        Ok((Err(err), log)) => (
            StatusCode::OK,
            Json(ServerTestResponse {
                ok: false,
                message: format!("{err:#}"),
                detail: log,
            }),
        )
            .into_response(),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("连接测试任务失败: {err}"),
        )
        .into_response(),
    }
}

/// 采集单台服务器。共享一个 handler，避免 /api/metrics 与 /:id/metrics 两套逻辑。
fn collect_metrics(targets: Vec<models::DeployTarget>) -> Vec<ServerMetrics> {
    targets
        .into_iter()
        .map(|target| metrics::sample_server(&target))
        .collect()
}

async fn server_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let target = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(_) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                    .into_response()
            }
        };
        match deploy_target_for_server(&conn, id) {
            Ok(Some(target)) => target,
            Ok(None) => return json_error(StatusCode::NOT_FOUND, "服务器不存在").into_response(),
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        }
    };

    let sampled_at = now_ts();
    let metrics = match task::spawn_blocking(move || collect_metrics(vec![target])).await {
        Ok(list) => list,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("采样任务失败: {err}"))
                .into_response()
        }
    };

    (
        StatusCode::OK,
        Json(ServerMetricsResponse { metrics, sampled_at }),
    )
        .into_response()
}

async fn all_metrics(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(err) = authenticate(&headers, &state) {
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let targets = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(_) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned")
                    .into_response()
            }
        };
        let servers = match list_servers(&conn) {
            Ok(list) => list,
            Err(err) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response()
            }
        };
        let mut targets = Vec::new();
        for summary in servers {
            if !summary.server.enabled {
                continue;
            }
            match deploy_target_for_server(&conn, summary.server.id) {
                Ok(Some(target)) => targets.push(target),
                Ok(None) => {}
                Err(err) => {
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                        .into_response()
                }
            }
        }
        targets
    };

    let sampled_at = now_ts();
    let metrics = match task::spawn_blocking(move || collect_metrics(targets)).await {
        Ok(list) => list,
        Err(err) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("采样任务失败: {err}"))
                .into_response()
        }
    };

    (
        StatusCode::OK,
        Json(ServerMetricsResponse { metrics, sampled_at }),
    )
        .into_response()
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

    enqueue_project_deployment(state, project, deploy_id);

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
    if let Err(err) = verify_webhook_signature(&state, &headers, &body) {
        warn(&format!("Webhook signature failed: {err:#}"));
        return StatusCode::UNAUTHORIZED;
    }

    match webhook_event_name(&headers).unwrap_or("") {
        "push" => handle_push_event(&state, &body),
        "release" => handle_release_event(&state, &body),
        _ => StatusCode::NO_CONTENT,
    }
}

async fn handle_artifact_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(err) = verify_callback_signature(&state, &headers, &body) {
        warn(&format!("Artifact callback signature failed: {err:#}"));
        return json_error(StatusCode::UNAUTHORIZED, err.to_string()).into_response();
    }

    let payload: ArtifactReadyRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid callback payload: {err}"),
            )
            .into_response();
        }
    };

    let repository = payload.repository.trim();
    let branch = payload.branch.trim();
    let artifact_url = payload.artifact_url.trim();
    if repository.is_empty() || branch.is_empty() || artifact_url.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "repository、branch 和 artifact_url 不能为空",
        )
        .into_response();
    }
    if !(artifact_url.starts_with("https://") || artifact_url.starts_with("http://")) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "artifact_url 必须是 http:// 或 https:// 地址",
        )
        .into_response();
    }

    let artifact_sha256 = match payload
        .artifact_sha256
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_sha256)
        .transpose()
    {
        Ok(value) => value,
        Err(err) => {
            return json_error(StatusCode::BAD_REQUEST, err.to_string()).into_response();
        }
    };

    let projects = match projects_for_repository_branch(&state, repository, branch) {
        Ok(projects) => projects,
        Err(err) => {
            error(&format!("Callback project query failed: {err:#}"));
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    if projects.is_empty() {
        return json_error(StatusCode::NOT_FOUND, "没有匹配的已启用项目").into_response();
    }

    let projects = projects
        .into_iter()
        .filter(supports_remote_artifact)
        .collect::<Vec<_>>();
    if projects.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "没有匹配的可远程部署项目").into_response();
    }

    let mut deployment_ids = Vec::new();
    for project in projects {
        let project = apply_artifact_override(&project, artifact_url, artifact_sha256.as_deref());
        let message = artifact_ready_message(&payload);
        match queue_project_deployment(&state, project, branch, Some(&message)) {
            Ok(deploy_id) => deployment_ids.push(deploy_id),
            Err(err) => error(&format!("Queue callback deployment failed: {err:#}")),
        }
    }

    if deployment_ids.is_empty() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "部署任务创建失败").into_response();
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "message": "制品部署任务已创建",
            "deployment_ids": deployment_ids,
        })),
    )
        .into_response()
}

fn handle_push_event(state: &AppState, body: &[u8]) -> StatusCode {
    let payload: PushPayload = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(err) => {
            warn(&format!("Push webhook payload parse error: {err}"));
            return StatusCode::BAD_REQUEST;
        }
    };

    let Some(repository) = payload.repository.as_ref() else {
        warn("Push webhook is missing repository.full_name");
        return StatusCode::BAD_REQUEST;
    };
    let Some(branch) = extract_branch(&payload.git_ref) else {
        return StatusCode::BAD_REQUEST;
    };

    let projects = match projects_for_repository_branch(state, &repository.full_name, &branch) {
        Ok(projects) => projects,
        Err(err) => {
            error(&format!("Push webhook project query failed: {err:#}"));
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    if projects.is_empty() {
        log(&format!(
            "No enabled project for repository/branch: {}/{}",
            repository.full_name, branch
        ));
        return StatusCode::ACCEPTED;
    }

    for project in projects {
        let message = format!("Triggered by push webhook: {}", payload.git_ref);
        if let Err(err) = queue_project_deployment(state, project, &branch, Some(&message)) {
            error(&format!("Queue push deployment failed: {err:#}"));
        }
    }

    StatusCode::ACCEPTED
}

fn handle_release_event(state: &AppState, body: &[u8]) -> StatusCode {
    let payload: ReleaseWebhookPayload = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(err) => {
            warn(&format!("Release webhook payload parse error: {err}"));
            return StatusCode::BAD_REQUEST;
        }
    };

    if !matches!(
        payload.action.as_deref(),
        None | Some("published") | Some("released")
    ) {
        return StatusCode::NO_CONTENT;
    }

    let Some(branch) = payload
        .release
        .target_commitish
        .as_deref()
        .map(|value| value.strip_prefix("refs/heads/").unwrap_or(value))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        warn("Release webhook is missing release.target_commitish");
        return StatusCode::BAD_REQUEST;
    };

    let projects =
        match projects_for_repository_branch(state, &payload.repository.full_name, branch) {
            Ok(projects) => projects,
            Err(err) => {
                error(&format!("Release webhook project query failed: {err:#}"));
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        };

    if projects.is_empty() {
        log(&format!(
            "No enabled project for release repository/branch: {}/{}",
            payload.repository.full_name, branch
        ));
        return StatusCode::ACCEPTED;
    }

    for project in projects.into_iter().filter(supports_remote_artifact) {
        let Some(asset) = select_release_asset(&payload.release.assets) else {
            log(&format!(
                "No matching release asset for project: {}",
                project.name
            ));
            continue;
        };
        let sha256 = match asset
            .digest
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(digest) => match normalize_sha256(digest) {
                Ok(value) => Some(value),
                Err(err) => {
                    warn(&format!(
                        "Skipping release {} for project {}: invalid asset digest ({err})",
                        payload.release.tag_name, project.name
                    ));
                    continue;
                }
            },
            None => None,
        };
        let project =
            apply_artifact_override(&project, &asset.browser_download_url, sha256.as_deref());
        let message = format!(
            "Triggered by release {} ({})",
            payload.release.tag_name, asset.name
        );

        if let Err(err) = queue_project_deployment(state, project, branch, Some(&message)) {
            error(&format!("Queue release deployment failed: {err:#}"));
        }
    }

    StatusCode::ACCEPTED
}

fn projects_for_repository_branch(
    state: &AppState,
    repository: &str,
    branch: &str,
) -> Result<Vec<Project>> {
    let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
    let projects = list_projects_by_branch(&conn, branch)?;
    let repository_key = normalize_repository(repository);

    Ok(projects
        .into_iter()
        .filter(|project| normalize_repository(&project.repo_path) == repository_key)
        .collect())
}

fn queue_project_deployment(
    state: &AppState,
    project: Project,
    branch: &str,
    message: Option<&str>,
) -> Result<i64> {
    let deploy_id = {
        let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
        insert_deployment(&conn, project.id, branch, "queued", message)?
    };

    enqueue_project_deployment(state.clone(), project, deploy_id);
    Ok(deploy_id)
}

fn apply_artifact_override(
    project: &Project,
    artifact_url: &str,
    artifact_sha256: Option<&str>,
) -> Project {
    let mut project = project.clone();
    project.artifact_url = Some(artifact_url.to_string());
    project.artifact_sha256 = artifact_sha256
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    project
}

fn supports_remote_artifact(project: &Project) -> bool {
    project.deploy_mode == "ci_ssh"
        && project
            .ssh_host
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

fn artifact_ready_message(payload: &ArtifactReadyRequest) -> String {
    let mut message = String::from("Triggered by CI artifact callback");
    if let Some(tag) = payload
        .tag
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        message.push_str(&format!(", tag={tag}"));
    }
    if let Some(commit) = payload
        .commit
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        message.push_str(&format!(", commit={commit}"));
    }
    if let Some(custom) = payload
        .message
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        message.push_str(&format!(", {custom}"));
    }
    message
}

fn select_release_asset(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    if assets.len() == 1 {
        assets.first()
    } else {
        None
    }
}

fn normalize_repository(value: &str) -> String {
    let mut text = value
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();

    if let Some((_, rest)) = text.split_once("://") {
        text = rest.to_string();
    } else if let Some((_, rest)) = text.split_once('@') {
        text = rest.replace(':', "/");
    }

    let parts = text
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    parts
        .into_iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("/")
        .to_ascii_lowercase()
}

fn normalize_sha256(value: &str) -> Result<String> {
    let digest = value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase();

    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("artifact_sha256 必须是 64 位十六进制 SHA-256");
    }

    Ok(digest)
}

fn parse_deploy_concurrency(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(4)
        .clamp(1, 32)
}

/// Empty webhook secrets are only tolerable when nothing outside this host can
/// reach the listener.
fn bind_is_loopback(bind: &str) -> bool {
    bind.parse::<SocketAddr>()
        .map(|address| address.ip().is_loopback())
        .unwrap_or_else(|_| bind.starts_with("localhost"))
}

const DEPLOY_DRAIN_TIMEOUT: Duration = Duration::from_secs(120);

/// Resolves on SIGINT or, on unix, SIGTERM.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }

    log("Shutdown signal received: no longer accepting new requests.");
}

/// In-flight deployments run on blocking threads that graceful shutdown does not
/// wait for, so give them a bounded chance to finish before the process exits.
async fn drain_running_deployments(state: &AppState, concurrency: usize) {
    let deadline = tokio::time::Instant::now() + DEPLOY_DRAIN_TIMEOUT;

    while state.deploy_semaphore.available_permits() < concurrency {
        if tokio::time::Instant::now() >= deadline {
            warn(&format!(
                "Deployment still running after {}s; exiting anyway.",
                DEPLOY_DRAIN_TIMEOUT.as_secs()
            ));
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    log("All running deployments finished.");
}

fn enqueue_project_deployment(state: AppState, project: Project, deploy_id: i64) {
    tokio::spawn(async move {
        let project_lock = match state.project_locks.lock() {
            Ok(mut locks) => locks
                .entry(project.id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone(),
            Err(_) => {
                let _ = update_deployment_status(
                    &state,
                    deploy_id,
                    "failed",
                    Some("Project lock map poisoned".to_string()),
                    Some("project lock map poisoned".to_string()),
                );
                return;
            }
        };

        let _project_guard = project_lock.lock_owned().await;
        let _permit = match state.deploy_semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                let _ = update_deployment_status(
                    &state,
                    deploy_id,
                    "failed",
                    Some("Deployment queue is closed".to_string()),
                    Some("deployment queue is closed".to_string()),
                );
                return;
            }
        };

        let worker_state = state.clone();
        if let Err(err) =
            task::spawn_blocking(move || run_project_deployment(worker_state, project, deploy_id))
                .await
        {
            error(&format!("Deployment task panicked: {err:#}"));
            let _ = update_deployment_status(
                &state,
                deploy_id,
                "failed",
                Some("Deployment task panicked".to_string()),
                Some(format!("deployment task panicked: {err}")),
            );
        }
    });
}

fn run_project_deployment(state: AppState, project: Project, deploy_id: i64) {
    let _ = update_deployment_status(
        &state,
        deploy_id,
        "running",
        Some("部署进行中".to_string()),
        Some(String::new()),
    );

    let mut deploy_log = String::new();
    let run_result = check_and_deploy_project(
        &state,
        &project,
        &project.branch,
        deploy_id,
        &mut deploy_log,
    );

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
            error(&msg);
            let _ =
                update_deployment_status(&state, deploy_id, "failed", Some(msg), Some(deploy_log));
        }
    }
}

fn check_and_deploy_project(
    state: &AppState,
    project: &Project,
    branch: &str,
    deploy_id: i64,
    deploy_log: &mut String,
) -> Result<bool> {
    match project.deploy_mode.as_str() {
        "ci_ssh" => return deploy::deploy_remote_artifact(project, deploy_id, deploy_log),
        "local" => {}
        mode => bail!("不支持的部署方式: {mode}"),
    }

    log_line(deploy_log, &format!("Start project: {}", project.name));
    let repository = prepare_repository(state, project, deploy_log)?;
    let repo_path = repository.path.as_path();

    if repository.cloned {
        log_line(
            deploy_log,
            "Repository cloned for the first time; building immediately.",
        );
    } else {
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

        if repository.managed {
            git_checkout_remote(&state.git_path, repo_path, branch, deploy_log)?;
        } else {
            git_pull(&state.git_path, repo_path, branch, deploy_log)?;
        }
    }

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

struct PreparedRepository {
    path: PathBuf,
    cloned: bool,
    managed: bool,
}

fn prepare_repository(
    state: &AppState,
    project: &Project,
    deploy_log: &mut String,
) -> Result<PreparedRepository> {
    let source = project.repo_path.trim();
    if !is_remote_git_url(source) {
        debug(&format!("Using local repository: {source}"));
        return Ok(PreparedRepository {
            path: PathBuf::from(source),
            cloned: false,
            managed: false,
        });
    }

    let destination = state.workspace_root.join(format!("project-{}", project.id));
    if destination.join(".git").is_dir() {
        log_line(
            deploy_log,
            &format!("Using managed repository: {}", destination.display()),
        );
        run_cmd(
            Command::new(&*state.git_path)
                .arg("-C")
                .arg(&destination)
                .arg("remote")
                .arg("set-url")
                .arg("origin")
                .arg(source),
            deploy_log,
        )
        .context("update managed repository origin failed")?;
        return Ok(PreparedRepository {
            path: destination,
            cloned: false,
            managed: true,
        });
    }

    if destination.exists() {
        bail!(
            "Managed repository path exists but is not a Git repository: {}",
            destination.display()
        );
    }

    log_line(
        deploy_log,
        &format!("Cloning {} into {}", source, destination.display()),
    );
    run_cmd(
        Command::new(&*state.git_path)
            .arg("clone")
            .arg("--branch")
            .arg(&project.branch)
            .arg("--single-branch")
            .arg(source)
            .arg(&destination),
        deploy_log,
    )
    .context("git clone failed")?;

    Ok(PreparedRepository {
        path: destination,
        cloned: true,
        managed: true,
    })
}

fn is_remote_git_url(value: &str) -> bool {
    value.starts_with("https://")
        || value.starts_with("http://")
        || value.starts_with("ssh://")
        || value.starts_with("git://")
        || value.starts_with("git@")
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

fn git_checkout_remote(
    git_path: &FsPath,
    repo: &FsPath,
    branch: &str,
    deploy_log: &mut String,
) -> Result<()> {
    run_cmd(
        Command::new(git_path)
            .arg("-C")
            .arg(repo)
            .arg("checkout")
            .arg("-B")
            .arg(branch)
            .arg(format!("origin/{branch}")),
        deploy_log,
    )
    .context("git checkout remote branch failed")
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
        return deploy_directory(src, dst);
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

    replace_file(src, &final_dst)
}

/// Copies the directory next to its target and swaps it in with renames, so the
/// live directory is never left half written. The previous release is kept next
/// to it as `.<name>.previous` for manual rollback.
fn deploy_directory(src: &FsPath, dst: &FsPath) -> Result<()> {
    if dst.exists() && !dst.is_dir() {
        bail!(
            "Deploy target must be directory for dir source: {}",
            dst.display()
        );
    }

    let parent = dst
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("Deploy target has no parent directory: {}", dst.display()))?;
    let name = dst
        .file_name()
        .ok_or_else(|| anyhow!("Deploy target has no file name: {}", dst.display()))?
        .to_string_lossy()
        .into_owned();

    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create deploy parent dir: {}", parent.display()))?;

    let staging = parent.join(format!(".{name}.staging-{}", std::process::id()));
    let previous = parent.join(format!(".{name}.previous"));

    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("Failed to clear staging dir: {}", staging.display()))?;
    }
    fs::create_dir_all(&staging)
        .with_context(|| format!("Failed to create staging dir: {}", staging.display()))?;

    if let Err(err) = copy_dir_all(src, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    if previous.exists() {
        fs::remove_dir_all(&previous)
            .with_context(|| format!("Failed to clear previous dir: {}", previous.display()))?;
    }
    if dst.exists() {
        fs::rename(dst, &previous).with_context(|| {
            format!(
                "Failed to move current deploy aside: {} -> {}",
                dst.display(),
                previous.display()
            )
        })?;
    }

    if let Err(err) = fs::rename(&staging, dst) {
        // Put the previous release back so a failed swap does not leave the
        // deploy path missing.
        let _ = fs::rename(&previous, dst);
        let _ = fs::remove_dir_all(&staging);
        return Err(err)
            .with_context(|| format!("Failed to activate deploy dir: {}", dst.display()));
    }

    Ok(())
}

/// Writes next to the target before swapping, so a failed copy cannot truncate
/// the live file.
fn replace_file(src: &FsPath, dst: &FsPath) -> Result<()> {
    let name = dst
        .file_name()
        .ok_or_else(|| anyhow!("Deploy target has no file name: {}", dst.display()))?;
    let temp = dst.with_file_name(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));

    fs::copy(src, &temp).with_context(|| {
        format!(
            "Failed to copy from {} to {}",
            src.display(),
            temp.display()
        )
    })?;

    #[cfg(windows)]
    if dst.exists() {
        fs::remove_file(dst)
            .with_context(|| format!("Failed to replace existing file: {}", dst.display()))?;
    }

    if let Err(err) = fs::rename(&temp, dst) {
        let _ = fs::remove_file(&temp);
        return Err(err)
            .with_context(|| format!("Failed to activate deploy file: {}", dst.display()));
    }

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

fn webhook_event_name(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("X-GitHub-Event")
        .or_else(|| headers.get("X-Gitea-Event"))
        .and_then(|value| value.to_str().ok())
}

fn webhook_signature(headers: &HeaderMap) -> Option<(&'static str, &str)> {
    if let Some(value) = headers
        .get("X-Auto-Deploy-Signature")
        .and_then(|value| value.to_str().ok())
    {
        return Some((
            "X-Auto-Deploy-Signature",
            value.strip_prefix("sha256=").unwrap_or(value),
        ));
    }

    if let Some(value) = headers
        .get("X-Hub-Signature-256")
        .and_then(|value| value.to_str().ok())
    {
        return Some((
            "X-Hub-Signature-256",
            value.strip_prefix("sha256=").unwrap_or(value),
        ));
    }

    headers
        .get("X-Gitea-Signature")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            (
                "X-Gitea-Signature",
                value.strip_prefix("sha256=").unwrap_or(value),
            )
        })
}

fn verify_webhook_signature(state: &AppState, headers: &HeaderMap, body: &[u8]) -> Result<()> {
    let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
    let secret = get_setting(&conn, "webhook_secret")?.unwrap_or_default();
    drop(conn);

    if secret.is_empty() {
        return Ok(());
    }

    verify_hmac_signature(&secret, headers, body)
}

fn verify_callback_signature(state: &AppState, headers: &HeaderMap, body: &[u8]) -> Result<()> {
    let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
    let secret = get_setting(&conn, "webhook_secret")?.unwrap_or_default();
    drop(conn);

    if secret.is_empty() {
        bail!("webhook_secret 未配置，CI 回调接口已禁用");
    }

    verify_hmac_signature(&secret, headers, body)
}

fn verify_hmac_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> Result<()> {
    let (header_name, sig) =
        webhook_signature(headers).with_context(|| "Missing webhook signature header")?;
    let sig_bytes = hex::decode(sig).context("Invalid signature hex")?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).context("Invalid secret")?;
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.ct_eq(&sig_bytes).unwrap_u8() != 1 {
        bail!("Signature mismatch from {header_name}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_deployment_concurrency_limit() {
        assert_eq!(parse_deploy_concurrency(None), 4);
        assert_eq!(parse_deploy_concurrency(Some("8")), 8);
        assert_eq!(parse_deploy_concurrency(Some("0")), 1);
        assert_eq!(parse_deploy_concurrency(Some("100")), 32);
        assert_eq!(parse_deploy_concurrency(Some("invalid")), 4);
    }

    #[test]
    fn extracts_push_branch() {
        assert_eq!(extract_branch("refs/heads/main").as_deref(), Some("main"));
        assert_eq!(extract_branch("refs/tags/v1.0.0"), None);
    }

    #[test]
    fn normalizes_repository_identifiers() {
        assert_eq!(
            normalize_repository("https://github.com/lurendie/auto_deployment.git"),
            "lurendie/auto_deployment"
        );
        assert_eq!(
            normalize_repository("git@github.com:lurendie/auto_deployment.git"),
            "lurendie/auto_deployment"
        );
        assert_eq!(
            normalize_repository("lurendie/auto_deployment"),
            "lurendie/auto_deployment"
        );
    }

    #[test]
    fn validates_artifact_sha256() {
        let sha = "a".repeat(64);
        assert_eq!(normalize_sha256(&format!("sha256:{sha}")).unwrap(), sha);
        assert!(normalize_sha256("not-a-sha").is_err());
    }

    #[test]
    fn detects_remote_git_urls() {
        assert!(is_remote_git_url("https://github.com/owner/repo.git"));
        assert!(is_remote_git_url("git@github.com:owner/repo.git"));
        assert!(is_remote_git_url("ssh://git@example.com/owner/repo.git"));
        assert!(!is_remote_git_url("D:\\repos\\demo"));
        assert!(!is_remote_git_url("/srv/repos/demo"));
    }

    #[test]
    fn selects_only_release_asset() {
        let assets = vec![ReleaseAsset {
            name: "app.zip".to_string(),
            browser_download_url: "https://example.com/app.zip".to_string(),
            digest: Some(format!("sha256:{}", "a".repeat(64))),
        }];

        let selected = select_release_asset(&assets).unwrap();
        assert_eq!(selected.name, "app.zip");
    }

    #[test]
    fn verifies_hmac_callback_signature() {
        let secret = "callback-secret";
        let body = br#"{"repository":"owner/repo"}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Auto-Deploy-Signature",
            format!("sha256={signature}").parse().unwrap(),
        );

        assert!(verify_hmac_signature(secret, &headers, body).is_ok());

        headers.insert("X-Auto-Deploy-Signature", "sha256=00".parse().unwrap());
        assert!(verify_hmac_signature(secret, &headers, body).is_err());
    }

    #[test]
    fn directory_deploy_swaps_atomically_and_keeps_previous() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("build");
        let dst = tmp.path().join("app");

        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("index.html"), b"v1").unwrap();
        fs::write(src.join("nested/app.js"), b"v1").unwrap();
        deploy_artifact(&src, &dst).unwrap();

        assert_eq!(fs::read(dst.join("index.html")).unwrap(), b"v1");
        assert_eq!(fs::read(dst.join("nested/app.js")).unwrap(), b"v1");

        // Second release drops a file; the swap must not merge with the old tree.
        fs::remove_file(src.join("nested/app.js")).unwrap();
        fs::remove_dir(src.join("nested")).unwrap();
        fs::write(src.join("index.html"), b"v2").unwrap();
        deploy_artifact(&src, &dst).unwrap();

        assert_eq!(fs::read(dst.join("index.html")).unwrap(), b"v2");
        assert!(!dst.join("nested").exists());
        assert_eq!(
            fs::read(tmp.path().join(".app.previous/index.html")).unwrap(),
            b"v1"
        );

        let leftovers = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("staging"))
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty(), "staging leftovers: {leftovers:?}");
    }

    #[test]
    fn file_deploy_replaces_target_without_leaving_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("artifact.bin");
        let dst = tmp.path().join("deployed.bin");

        fs::write(&src, b"v1").unwrap();
        deploy_artifact(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"v1");

        fs::write(&src, b"v2").unwrap();
        deploy_artifact(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"v2");

        let leftovers = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("tmp-"))
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty(), "temp leftovers: {leftovers:?}");
    }
}
