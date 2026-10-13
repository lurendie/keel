use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use ssh2::{CheckResult, KnownHostFileKind, Session};
use std::{
    env,
    fs::File,
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::NamedTempFile;

use crate::{
    db::deploy_target_for_project,
    models::{DeployTarget, Project},
    utils::log,
};

const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const SSH_HANDSHAKE_TIMEOUT_MS: u32 = 30_000;
const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const SSH_KNOWN_HOSTS_ENV: &str = "SSH_KNOWN_HOSTS";

pub fn deploy_remote_artifact(
    project: &Project,
    deploy_id: i64,
    deploy_log: &mut String,
) -> Result<bool> {
    let artifact_url = project
        .artifact_url
        .as_deref()
        .context("artifact_url 未配置")?;
    let target = deploy_target_for_project(project)
        .context("项目未选择服务器，或服务器缺少主机 / 登录用户")?;

    log_line(
        deploy_log,
        &format!("Start remote deploy: {} -> {}", project.name, artifact_url),
    );

    let artifact = download_artifact(artifact_url, project.artifact_sha256.as_deref(), deploy_log)?;
    let session = connect_target(&target, deploy_log)?;

    let remote_base = format!("/tmp/auto-deploy/{}/{}", project.id, deploy_id);
    run_remote_shell(
        &session,
        &format!("mkdir -p {}", shell_quote(&remote_base)),
        deploy_log,
    )?;

    if let Some(cmd) = &project.stop_command {
        run_remote_shell(&session, cmd, deploy_log)?;
    }

    let artifact_name = file_name_from_url(artifact_url);
    let remote_archive = PathBuf::from(&remote_base).join(&artifact_name);
    upload_file(&session, artifact.path(), &remote_archive, deploy_log)?;
    deploy_uploaded_artifact(&session, &remote_archive, &project.deploy_path, deploy_log)?;

    if let Some(cmd) = &project.start_command {
        run_remote_shell(&session, cmd, deploy_log)?;
    }

    log_line(
        deploy_log,
        &format!("Remote deploy completed: {}", project.name),
    );
    Ok(true)
}

fn download_artifact(
    artifact_url: &str,
    expected_sha256: Option<&str>,
    deploy_log: &mut String,
) -> Result<NamedTempFile> {
    let client = Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(HTTP_REQUEST_TIMEOUT)
        .build()
        .context("create http client failed")?;
    let mut response = client
        .get(artifact_url)
        .send()
        .with_context(|| format!("download artifact failed: {artifact_url}"))?
        .error_for_status()
        .with_context(|| format!("artifact response not ok: {artifact_url}"))?;

    if let Some(length) = response.content_length() {
        if length > MAX_ARTIFACT_BYTES {
            bail!("artifact is too large: {length} bytes, limit is {MAX_ARTIFACT_BYTES} bytes");
        }
    }

    let mut artifact = NamedTempFile::new().context("create temp file failed")?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut written: u64 = 0;

    loop {
        let n = response
            .read(&mut buf)
            .context("read artifact stream failed")?;
        if n == 0 {
            break;
        }
        written += n as u64;
        if written > MAX_ARTIFACT_BYTES {
            bail!("artifact exceeds {MAX_ARTIFACT_BYTES} bytes limit");
        }
        artifact
            .write_all(&buf[..n])
            .context("write artifact stream failed")?;
        hasher.update(&buf[..n]);
    }

    artifact.as_file().sync_all().ok();

    let actual_sha256 = hex::encode(hasher.finalize());
    log_line(
        deploy_log,
        &format!("Artifact downloaded: {artifact_url}, sha256={actual_sha256}"),
    );

    if let Some(expected) = expected_sha256.map(|v| v.trim()).filter(|v| !v.is_empty()) {
        if expected.to_lowercase() != actual_sha256 {
            bail!(
                "artifact sha256 mismatch: expected={}, actual={}",
                expected,
                actual_sha256
            );
        }
    }

    Ok(artifact)
}

/// 建立一条已通过主机公钥校验、并完成认证的 SSH 会话。
///
/// 认证方式由 `target.auth_method` 决定：`password` 走口令登录，其余走公钥。
/// 两种方式都必须先过 `verify_host_key`，否则密码可能被中间人截获。
pub fn connect_target(target: &DeployTarget, deploy_log: &mut String) -> Result<Session> {
    log_line(
        deploy_log,
        &format!(
            "Connecting SSH: {}@{}:{} ({})",
            target.user, target.host, target.port, target.auth_method
        ),
    );

    let port = u16::try_from(target.port)
        .ok()
        .filter(|port| *port > 0)
        .with_context(|| format!("ssh_port 必须在 1-65535 之间: {}", target.port))?;

    let address = resolve_ssh_address(&target.host, port)?;
    let tcp = TcpStream::connect_timeout(&address, SSH_CONNECT_TIMEOUT)
        .with_context(|| format!("connect ssh failed: {}:{}", target.host, port))?;
    let mut session = Session::new().context("create ssh session failed")?;
    session.set_tcp_stream(tcp);
    session.set_timeout(SSH_HANDSHAKE_TIMEOUT_MS);
    session.handshake().context("ssh handshake failed")?;
    verify_host_key(&session, &target.host, port, deploy_log)?;

    match target.auth_method.as_str() {
        "password" => {
            let password = target
                .password
                .as_deref()
                .filter(|value| !value.is_empty())
                .context("服务器未配置登录密码")?;
            session
                .userauth_password(&target.user, password)
                .context("ssh password auth failed")?;
        }
        _ => {
            let key_path = target
                .private_key_path
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .context("服务器未配置私钥路径")?;
            session
                .userauth_pubkey_file(
                    &target.user,
                    None,
                    Path::new(key_path),
                    target.private_key_passphrase.as_deref(),
                )
                .context("ssh key auth failed")?;
        }
    }

    if !session.authenticated() {
        bail!(
            "ssh authentication failed: {}@{}:{}",
            target.user,
            target.host,
            port
        );
    }
    log_line(deploy_log, "SSH authenticated");

    // Stop/start and unpack commands can stay silent for minutes, so the read
    // timeout is limited to connection establishment.
    session.set_timeout(0);

    Ok(session)
}

/// 只做连接与认证，用来验证服务器配置是否可用。
pub fn test_connection(target: &DeployTarget, deploy_log: &mut String) -> Result<()> {
    let session = connect_target(target, deploy_log)?;
    // 一次最小往返，确认通道真的能用，而不只是握手成功。
    let mut channel = session
        .channel_session()
        .context("open ssh channel failed")?;
    channel.exec("true").context("exec probe failed")?;
    let exit = channel
        .exit_status()
        .context("get remote exit status failed")?;
    if exit != 0 {
        bail!("远程探测命令返回非零退出码: {exit}");
    }
    Ok(())
}

/// The SSH host key must match `known_hosts` before any credential is sent,
/// otherwise a man-in-the-middle could impersonate the deploy target.
fn verify_host_key(
    session: &Session,
    host: &str,
    port: u16,
    deploy_log: &mut String,
) -> Result<()> {
    let path = known_hosts_path();
    let (key, key_type) = session
        .host_key()
        .context("ssh server did not present a host key")?;
    let hint = format!(
        "请先执行 `ssh-keyscan -p {port} {host} >> {}` 并核对指纹，或用 SSH_KNOWN_HOSTS 指定文件。",
        path.display()
    );

    let mut known_hosts = session
        .known_hosts()
        .context("create known_hosts context failed")?;
    known_hosts
        .read_file(&path, KnownHostFileKind::OpenSSH)
        .with_context(|| format!("读取 known_hosts 失败: {}。{hint}", path.display()))?;

    match known_hosts.check_port(host, port, key) {
        CheckResult::Match => {
            log_line(
                deploy_log,
                &format!("SSH host key verified: {host}:{port} ({key_type:?})"),
            );
            Ok(())
        }
        CheckResult::NotFound => bail!(
            "目标主机公钥不在 {} 中: {host}:{port}。{hint}",
            path.display()
        ),
        CheckResult::Mismatch => bail!(
            "SSH 主机密钥与 {} 中记录不一致: {host}:{port}，可能存在中间人攻击，已中止部署。",
            path.display()
        ),
        CheckResult::Failure => bail!("SSH 主机密钥校验失败: {host}:{port}"),
    }
}

fn known_hosts_path() -> PathBuf {
    if let Ok(value) = env::var(SSH_KNOWN_HOSTS_ENV) {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }

    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".ssh").join("known_hosts"))
        .unwrap_or_else(|_| PathBuf::from("known_hosts"))
}

fn resolve_ssh_address(host: &str, port: u16) -> Result<SocketAddr> {
    let mut addresses = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("resolve ssh host failed: {host}"))?;
    addresses
        .next()
        .ok_or_else(|| anyhow!("ssh host did not resolve to any address: {host}"))
}

fn upload_file(
    session: &Session,
    local_path: &Path,
    remote_path: &Path,
    deploy_log: &mut String,
) -> Result<()> {
    let remote_parent = remote_path
        .parent()
        .ok_or_else(|| anyhow!("remote path has no parent: {}", remote_path.display()))?;

    run_remote_shell(
        session,
        &format!(
            "mkdir -p {}",
            shell_quote(&remote_parent.display().to_string())
        ),
        deploy_log,
    )?;

    let sftp = session.sftp().context("open sftp failed")?;
    let mut local_file = File::open(local_path)
        .with_context(|| format!("open local artifact failed: {}", local_path.display()))?;
    let mut remote_file = sftp
        .create(remote_path)
        .with_context(|| format!("create remote file failed: {}", remote_path.display()))?;
    std::io::copy(&mut local_file, &mut remote_file).context("upload artifact failed")?;
    log_line(
        deploy_log,
        &format!(
            "Artifact uploaded: {} -> {}",
            local_path.display(),
            remote_path.display()
        ),
    );
    Ok(())
}

fn deploy_uploaded_artifact(
    session: &Session,
    remote_archive: &Path,
    remote_deploy_path: &str,
    deploy_log: &mut String,
) -> Result<()> {
    let archive_text = remote_archive.to_string_lossy().to_string();
    let deploy_path = shell_quote(remote_deploy_path);
    let archive_path = shell_quote(&archive_text);

    let command = if archive_text.ends_with(".zip") {
        format!("mkdir -p {deploy_path} && unzip -o {archive_path} -d {deploy_path}")
    } else if archive_text.ends_with(".tar.gz") || archive_text.ends_with(".tgz") {
        format!("mkdir -p {deploy_path} && tar -xzf {archive_path} -C {deploy_path}")
    } else {
        format!("mkdir -p {deploy_path} && cp {archive_path} {deploy_path}/")
    };

    run_remote_shell(session, &command, deploy_log)
}

fn run_remote_shell(session: &Session, cmd: &str, deploy_log: &mut String) -> Result<()> {
    run_remote_command(session, &format!("sh -lc {}", shell_quote(cmd)), deploy_log)
}

fn run_remote_command(session: &Session, cmd: &str, deploy_log: &mut String) -> Result<()> {
    log_line(deploy_log, &format!("$ ssh {cmd}"));

    let mut channel = session
        .channel_session()
        .context("open ssh channel failed")?;
    channel.exec(cmd).context("exec remote command failed")?;

    let mut stdout = String::new();
    let mut stderr = String::new();
    channel
        .read_to_string(&mut stdout)
        .context("read remote stdout failed")?;
    channel
        .stderr()
        .read_to_string(&mut stderr)
        .context("read remote stderr failed")?;
    channel.wait_close().context("wait remote command failed")?;
    let exit = channel
        .exit_status()
        .context("get remote exit status failed")?;

    if !stdout.trim().is_empty() {
        log_line(deploy_log, &format!("stdout:\n{}", stdout.trim()));
    }
    if !stderr.trim().is_empty() {
        log_line(deploy_log, &format!("stderr:\n{}", stderr.trim()));
    }

    if exit != 0 {
        bail!("remote command failed with exit code {exit}");
    }

    Ok(())
}

fn file_name_from_url(url: &str) -> String {
    let without_query = url.split('?').next().unwrap_or(url);
    without_query
        .rsplit('/')
        .next()
        .filter(|v| !v.is_empty())
        .unwrap_or("artifact.bin")
        .to_string()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn log_line(deploy_log: &mut String, line: &str) {
    deploy_log.push_str(line);
    deploy_log.push('\n');
    log(line);
}
