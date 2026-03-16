use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::{
    fs::File,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

use crate::{models::Project, utils::log};

pub fn deploy_remote_artifact(
    project: &Project,
    deploy_id: i64,
    deploy_log: &mut String,
) -> Result<bool> {
    let artifact_url = project
        .artifact_url
        .as_deref()
        .context("artifact_url 未配置")?;
    let ssh_host = project.ssh_host.as_deref().context("ssh_host 未配置")?;
    let ssh_user = project.ssh_user.as_deref().context("ssh_user 未配置")?;
    let ssh_key_path = project
        .ssh_private_key_path
        .as_deref()
        .context("ssh_private_key_path 未配置")?;
    let ssh_port = project.ssh_port.unwrap_or(22) as u16;

    log_line(
        deploy_log,
        &format!("Start remote deploy: {} -> {}", project.name, artifact_url),
    );

    let artifact = download_artifact(artifact_url, project.artifact_sha256.as_deref(), deploy_log)?;
    let session = connect_ssh(
        ssh_host,
        ssh_port,
        ssh_user,
        ssh_key_path,
        project.ssh_private_key_passphrase.as_deref(),
        deploy_log,
    )?;

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
        .build()
        .context("create http client failed")?;
    let mut response = client
        .get(artifact_url)
        .send()
        .with_context(|| format!("download artifact failed: {artifact_url}"))?
        .error_for_status()
        .with_context(|| format!("artifact response not ok: {artifact_url}"))?;

    let mut artifact = NamedTempFile::new().context("create temp file failed")?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = response
            .read(&mut buf)
            .context("read artifact stream failed")?;
        if n == 0 {
            break;
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

fn connect_ssh(
    host: &str,
    port: u16,
    user: &str,
    private_key_path: &str,
    passphrase: Option<&str>,
    deploy_log: &mut String,
) -> Result<Session> {
    log_line(deploy_log, &format!("Connecting SSH: {user}@{host}:{port}"));

    let tcp = TcpStream::connect((host, port))
        .with_context(|| format!("connect ssh failed: {host}:{port}"))?;
    let mut session = Session::new().context("create ssh session failed")?;
    session.set_tcp_stream(tcp);
    session.handshake().context("ssh handshake failed")?;
    session
        .userauth_pubkey_file(user, None, Path::new(private_key_path), passphrase)
        .context("ssh key auth failed")?;
    if !session.authenticated() {
        bail!("ssh authentication failed");
    }

    Ok(session)
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
