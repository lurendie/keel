use anyhow::{bail, Context, Result};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use hmac::{Hmac, Mac};
use lettre::message::Mailbox;
use lettre::{Message, SmtpTransport, Transport};
use serde::Deserialize;
use sha2::Sha256;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};
use subtle::ConstantTimeEq;
use tokio::task;

#[derive(Debug, Deserialize, Clone)]
struct Config {
    webhook_bind: Option<String>,
    webhook_path: Option<String>,
    webhook_secret: Option<String>,
    email: Option<EmailConfig>,
    projects: Vec<ProjectConfig>,
    git_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProjectConfig {
    name: Option<String>,
    repo_path: PathBuf,
    branches: Option<Vec<String>>,
    steps: Vec<DeployStep>,
}

#[derive(Debug, Deserialize, Clone)]
struct DeployStep {
    name: Option<String>,
    workdir: Option<PathBuf>,
    build_command: String,
    built_path: PathBuf,
    deploy_path: PathBuf,
    stop_command: Option<String>,
    start_command: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct EmailConfig {
    smtp_server: String,
    smtp_port: Option<u16>,
    smtp_username: String,
    smtp_password: String,
    from: String,
    to: Vec<String>,
    subject_prefix: Option<String>,
}

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    deploy_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Deserialize)]
struct PushPayload {
    #[serde(rename = "ref")]
    git_ref: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::var("AUTO_DEPLOY_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let config = load_config(&config_path)?;
    log(&format!("Loaded config from {}.", config_path));
    if config.projects.is_empty() {
        bail!("Config must include at least one project in [[projects]]");
    }

    let bind = config
        .webhook_bind
        .clone()
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());
    let path = config
        .webhook_path
        .clone()
        .unwrap_or_else(|| "/webhook".to_string());

    let state = AppState {
        config: Arc::new(config),
        deploy_lock: Arc::new(Mutex::new(())),
    };

    let app = Router::new()
        .route(&path, post(handle_webhook))
        .with_state(state);

    log(&format!(
        "Listening for GitHub webhooks at http://{}{}",
        bind, path
    ));
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("Failed to bind to {}", bind))?;
    axum::serve(listener, app).await.context("Server failed")?;
    Ok(())
}

fn load_config(path: &str) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path))?;
    let cfg = toml::from_str::<Config>(&content)
        .with_context(|| format!("Failed to parse config file: {}", path))?;
    Ok(cfg)
}

fn check_and_deploy_project(
    config: &Config,
    project: &ProjectConfig,
    branch: &str,
) -> Result<bool> {
    git_fetch(&project.repo_path, branch, config)?;

    let local = git_rev(&project.repo_path, "HEAD", config)?;
    let remote = git_rev(&project.repo_path, &format!("origin/{}", branch), config)?;

    if local == remote {
        log("No updates found.");
        return Ok(false);
    }

    log(&format!("Update detected. {} -> {}", local, remote));
    git_pull(&project.repo_path, branch, config)?;
    if project.steps.is_empty() {
        bail!("Project {} has no steps configured", project_label(project));
    }

    for step in &project.steps {
        let workdir = resolve_path(&project.repo_path, step.workdir.as_ref());
        log(&format!(
            "Running step {} in {}",
            step_label(step),
            workdir.display()
        ));
        run_shell(&step.build_command, Some(&workdir))?;

        if let Some(cmd) = &step.stop_command {
            log("Stopping application...");
            run_shell(cmd, None)?;
        }

        let built_path = resolve_path(&project.repo_path, Some(&step.built_path));
        let deploy_path = resolve_path(&project.repo_path, Some(&step.deploy_path));
        deploy_executable(&built_path, &deploy_path)?;

        if let Some(cmd) = &step.start_command {
            log("Starting application...");
            run_shell(cmd, None)?;
        }
    }

    Ok(true)
}

fn git_fetch(repo: &Path, branch: &str, config: &Config) -> Result<()> {
    run_cmd(
        Command::new(git_program(config))
            .arg("-C")
            .arg(repo)
            .arg("fetch")
            .arg("origin")
            .arg(branch),
    )
    .context("git fetch failed")
}

fn git_pull(repo: &Path, branch: &str, config: &Config) -> Result<()> {
    run_cmd(
        Command::new(git_program(config))
            .arg("-C")
            .arg(repo)
            .arg("pull")
            .arg("--ff-only")
            .arg("origin")
            .arg(branch),
    )
    .context("git pull failed")
}

fn git_rev(repo: &Path, rev: &str, config: &Config) -> Result<String> {
    let output = Command::new(git_program(config))
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg(rev)
        .output()
        .with_context(|| format!("git rev-parse {} failed", rev))?;

    if !output.status.success() {
        bail!(
            "git rev-parse {} failed: {}",
            rev,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn deploy_executable(src: &Path, dst: &Path) -> Result<()> {
    let src_meta =
        fs::metadata(src).with_context(|| format!("Source path not found: {}", src.display()))?;

    if src_meta.is_dir() {
        if dst.exists() && !dst.is_dir() {
            bail!(
                "Deploy path must be a directory when source is a directory: {}",
                dst.display()
            );
        }
        if dst.exists() {
            fs::remove_dir_all(dst)
                .with_context(|| format!("Failed to remove deploy directory: {}", dst.display()))?;
        }
        fs::create_dir_all(dst)
            .with_context(|| format!("Failed to create deploy directory: {}", dst.display()))?;
        copy_dir_all(src, dst)?;
        log(&format!(
            "Deployed directory from {} to {}",
            src.display(),
            dst.display()
        ));
        return Ok(());
    }

    let dst_str = dst.to_string_lossy();
    let treat_as_dir = dst.exists() && dst.is_dir() || dst_str.ends_with(std::path::MAIN_SEPARATOR);
    let final_dst = if treat_as_dir {
        let file_name = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Source file has no file name: {}", src.display()))?;
        dst.join(file_name)
    } else {
        dst.to_path_buf()
    };

    if let Some(parent) = final_dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create deploy directory: {}", parent.display()))?;
    }
    fs::copy(src, &final_dst).with_context(|| {
        format!(
            "Failed to copy executable from {} to {}",
            src.display(),
            final_dst.display()
        )
    })?;
    log(&format!("Deployed executable to {}", final_dst.display()));
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
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

fn run_shell(cmd: &str, workdir: Option<&Path>) -> Result<()> {
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

    run_cmd(&mut command)
}

fn run_cmd(cmd: &mut Command) -> Result<()> {
    let output = cmd.output().context("Failed to spawn command")?;
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

fn format_command(cmd: &Command) -> String {
    let mut text = cmd.get_program().to_string_lossy().to_string();
    for arg in cmd.get_args() {
        text.push(' ');
        text.push_str(&arg.to_string_lossy());
    }
    text
}

fn git_program(config: &Config) -> PathBuf {
    match &config.git_path {
        Some(path) => path.clone(),
        None => PathBuf::from("git"),
    }
}

fn log(message: &str) {
    let now = chrono::Local::now();
    let ts = now.format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] {}", ts, message);
    println!("{}", line);
    write_log_line(&now, &line);
}

fn write_log_line(now: &chrono::DateTime<chrono::Local>, line: &str) {
    let log_dir = log_dir();
    let _ = fs::create_dir_all(&log_dir);

    cleanup_old_logs_if_needed(&log_dir, now.date_naive());

    let file_name = format!("auto_deploy-{}.log", now.format("%Y-%m-%d"));
    let log_path = log_dir.join(file_name);
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
}

fn log_dir() -> PathBuf {
    if let Ok(dir) = env::var("AUTO_DEPLOY_LOG_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("logs")
}

fn cleanup_old_logs_if_needed(log_dir: &Path, today: chrono::NaiveDate) {
    static LAST_CLEANUP: OnceLock<Mutex<Option<chrono::NaiveDate>>> = OnceLock::new();
    let guard = LAST_CLEANUP.get_or_init(|| Mutex::new(None));
    let mut last = guard.lock().unwrap();
    if last.map(|d| d >= today).unwrap_or(false) {
        return;
    }
    *last = Some(today);

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(60 * 60 * 24 * 7))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("log") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
    }
}

async fn handle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(err) = verify_github_signature(&headers, &body, &state.config.webhook_secret) {
        log(&format!("Signature verification failed: {:#}", err));
        return StatusCode::UNAUTHORIZED;
    }

    if let Some(event) = headers.get("X-GitHub-Event") {
        if event.to_str().unwrap_or("") != "push" {
            return StatusCode::NO_CONTENT;
        }
    }

    let payload: PushPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => {
            log(&format!("Invalid payload: {:#}", err));
            return StatusCode::BAD_REQUEST;
        }
    };

    let branch = match extract_branch(&payload.git_ref) {
        Some(b) => b,
        None => {
            log(&format!("Unsupported ref: {}", payload.git_ref));
            return StatusCode::BAD_REQUEST;
        }
    };

    let state_clone = state.clone();
    task::spawn_blocking(move || {
        let _guard = state_clone.deploy_lock.lock().ok();
        let mut any_updated = false;
        let mut had_error = false;
        for project in &state_clone.config.projects {
            if !branch_allowed(&project.branches, &branch) {
                continue;
            }
            match check_and_deploy_project(&state_clone.config, project, &branch) {
                Ok(updated) => {
                    if updated {
                        any_updated = true;
                        log(&format!(
                            "Deployment completed for {}.",
                            project_label(project)
                        ));
                    }
                }
                Err(err) => {
                    had_error = true;
                    log(&format!("ERROR ({}): {:#}", project_label(project), err));
                }
            }
        }

        if any_updated {
            let _ = send_email_notification(
                &state_clone.config.email,
                &format!("Deployment succeeded ({})", branch),
                &format!("Deployments completed for branch: {}", branch),
            );
        }
        if had_error {
            let _ = send_email_notification(
                &state_clone.config.email,
                &format!("Deployment failed ({})", branch),
                &format!("One or more deployments failed for branch: {}", branch),
            );
        }
    });

    StatusCode::ACCEPTED
}

fn verify_github_signature(
    headers: &HeaderMap,
    body: &[u8],
    secret: &Option<String>,
) -> Result<()> {
    let Some(secret) = secret else {
        return Ok(());
    };
    let sig = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .context("Missing X-Hub-Signature-256")?;
    let sig = sig
        .strip_prefix("sha256=")
        .context("Invalid signature prefix")?;
    let sig_bytes = hex::decode(sig).context("Invalid signature hex")?;

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).context("Invalid webhook secret")?;
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.ct_eq(&sig_bytes).unwrap_u8() != 1 {
        bail!("Signature mismatch");
    }
    Ok(())
}

fn extract_branch(git_ref: &str) -> Option<String> {
    git_ref.strip_prefix("refs/heads/").map(|s| s.to_string())
}

fn branch_allowed(allowed: &Option<Vec<String>>, branch: &str) -> bool {
    match allowed {
        None => true,
        Some(list) if list.is_empty() => true,
        Some(list) => list.iter().any(|b| b == branch),
    }
}

fn project_label(project: &ProjectConfig) -> String {
    match &project.name {
        Some(name) if !name.is_empty() => name.clone(),
        _ => project.repo_path.display().to_string(),
    }
}

fn step_label(step: &DeployStep) -> String {
    match &step.name {
        Some(name) if !name.is_empty() => name.clone(),
        _ => "step".to_string(),
    }
}

fn resolve_path(base: &Path, path: Option<&PathBuf>) -> PathBuf {
    match path {
        None => base.to_path_buf(),
        Some(p) if p.is_absolute() => p.clone(),
        Some(p) => base.join(p),
    }
}

fn send_email_notification(email: &Option<EmailConfig>, subject: &str, body: &str) -> Result<()> {
    let Some(cfg) = email else {
        return Ok(());
    };

    let from: Mailbox = cfg.from.parse().context("Invalid email.from")?;
    let mut builder = Message::builder()
        .from(from)
        .subject(build_subject(cfg, subject));
    for addr in &cfg.to {
        let to: Mailbox = addr.parse().context("Invalid email.to")?;
        builder = builder.to(to);
    }

    let message = builder
        .body(body.to_string())
        .context("Failed to build email")?;
    let port = cfg.smtp_port.unwrap_or(587);
    let creds = lettre::transport::smtp::authentication::Credentials::new(
        cfg.smtp_username.clone(),
        cfg.smtp_password.clone(),
    );

    let mailer = SmtpTransport::starttls_relay(&cfg.smtp_server)
        .with_context(|| format!("Invalid smtp server: {}", cfg.smtp_server))?
        .credentials(creds)
        .port(port)
        .build();

    mailer.send(&message).context("Failed to send email")?;
    Ok(())
}

fn build_subject(cfg: &EmailConfig, subject: &str) -> String {
    match &cfg.subject_prefix {
        Some(prefix) if !prefix.is_empty() => format!("{} {}", prefix, subject),
        _ => subject.to_string(),
    }
}
