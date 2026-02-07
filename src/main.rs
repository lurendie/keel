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
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
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
}

#[derive(Debug, Deserialize, Clone)]
struct ProjectConfig {
    name: Option<String>,
    repo_path: PathBuf,
    build_command: String,
    built_executable: PathBuf,
    deploy_path: PathBuf,
    stop_command: Option<String>,
    start_command: Option<String>,
    branches: Option<Vec<String>>,
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

fn check_and_deploy_project(project: &ProjectConfig, branch: &str) -> Result<bool> {
    git_fetch(&project.repo_path, branch)?;

    let local = git_rev(&project.repo_path, "HEAD")?;
    let remote = git_rev(&project.repo_path, &format!("origin/{}", branch))?;

    if local == remote {
        log("No updates found.");
        return Ok(false);
    }

    log(&format!("Update detected. {} -> {}", local, remote));
    git_pull(&project.repo_path, branch)?;
    run_shell(&project.build_command, Some(&project.repo_path))?;

    deploy_executable(&project.built_executable, &project.deploy_path)?;

    if let Some(cmd) = &project.stop_command {
        log("Stopping application...");
        run_shell(cmd, None)?;
    }
    if let Some(cmd) = &project.start_command {
        log("Starting application...");
        run_shell(cmd, None)?;
    }

    Ok(true)
}

fn git_fetch(repo: &Path, branch: &str) -> Result<()> {
    run_cmd(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("fetch")
            .arg("origin")
            .arg(branch),
    )
    .context("git fetch failed")
}

fn git_pull(repo: &Path, branch: &str) -> Result<()> {
    run_cmd(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("pull")
            .arg("--ff-only")
            .arg("origin")
            .arg(branch),
    )
    .context("git pull failed")
}

fn git_rev(repo: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
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
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create deploy directory: {}", parent.display()))?;
    }
    fs::copy(src, dst).with_context(|| {
        format!(
            "Failed to copy executable from {} to {}",
            src.display(),
            dst.display()
        )
    })?;
    log(&format!("Deployed executable to {}", dst.display()));
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

fn log(message: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("[{}] {}", ts, message);
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
            match check_and_deploy_project(project, &branch) {
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
