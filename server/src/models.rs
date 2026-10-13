use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    #[serde(default)]
    pub iat: usize,
    pub exp: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiUser {
    pub id: i64,
    pub username: String,
    pub last_login_ip: Option<String>,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub deploy_mode: String,
    pub repo_path: String,
    pub branch: String,
    pub workdir: Option<String>,
    pub build_command: String,
    pub built_path: String,
    #[serde(skip)]
    pub artifact_url: Option<String>,
    #[serde(skip)]
    pub artifact_sha256: Option<String>,
    /// 目标服务器。部署所需的 SSH 字段由 servers 表 JOIN 得来。
    pub server_id: Option<i64>,
    pub server_name: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<i64>,
    pub ssh_user: Option<String>,
    pub ssh_private_key_path: Option<String>,
    #[serde(skip_serializing)]
    pub ssh_private_key_passphrase: Option<String>,
    /// `key` 或 `password`，决定 deploy 阶段用哪种认证握手。
    pub ssh_auth_method: Option<String>,
    #[serde(skip_serializing)]
    pub ssh_password: Option<String>,
    pub ssh_private_key_passphrase_set: bool,
    pub ssh_password_set: bool,
    pub deploy_path: String,
    pub stop_command: Option<String>,
    pub start_command: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// SSH 连接配置。只在服务端流转，密码与私钥口令不会序列化给前端。
#[derive(Debug, Clone)]
pub struct DeployTarget {
    pub server_id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub user: String,
    pub auth_method: String,
    pub private_key_path: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub password: Option<String>,
    /// 采样磁盘用量时分区的参考路径。
    pub deploy_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub group_name: Option<String>,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_method: String,
    pub private_key_path: Option<String>,
    pub private_key_passphrase_set: bool,
    pub password_set: bool,
    pub deploy_path: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 列表用的视图：附带被多少项目引用，删除前能看清影响面。
#[derive(Debug, Serialize)]
pub struct ServerSummary {
    #[serde(flatten)]
    pub server: Server,
    pub project_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpsertServerRequest {
    pub name: String,
    #[serde(default)]
    pub group_name: Option<String>,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: i64,
    pub username: String,
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    #[serde(default)]
    pub private_key_path: Option<String>,
    /// 空字符串表示清除已保存的口令，None 表示保持原值。
    #[serde(default)]
    pub private_key_passphrase: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub deploy_path: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_ssh_port() -> i64 {
    22
}

fn default_auth_method() -> String {
    "key".to_string()
}

fn default_enabled() -> bool {
    true
}

/// 一次资源采样的结果。字段为 None 表示该项采样失败。
#[derive(Debug, Serialize, Default, Clone)]
pub struct ServerMetrics {
    pub server_id: i64,
    pub sampled_at: String,
    pub reachable: bool,
    pub error: Option<String>,
    pub cpu_percent: Option<f64>,
    pub load1: Option<f64>,
    pub load5: Option<f64>,
    pub load15: Option<f64>,
    pub cpu_cores: Option<i64>,
    pub mem_total_kb: Option<i64>,
    pub mem_used_kb: Option<i64>,
    pub mem_percent: Option<f64>,
    pub swap_total_kb: Option<i64>,
    pub swap_used_kb: Option<i64>,
    pub disk_total_kb: Option<i64>,
    pub disk_used_kb: Option<i64>,
    pub disk_percent: Option<f64>,
    pub disk_mount: Option<String>,
    pub uptime_seconds: Option<i64>,
    pub os: Option<String>,
    pub kernel: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ServerMetricsResponse {
    pub metrics: Vec<ServerMetrics>,
    pub sampled_at: String,
}

#[derive(Debug, Serialize)]
pub struct ServerTestResponse {
    pub ok: bool,
    pub message: String,
    /// 握手与认证过程的日志，失败时用来定位是公钥校验还是认证出的问题。
    pub detail: String,
}


#[derive(Debug, Serialize, Clone)]
pub struct Deployment {
    pub id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub branch: String,
    pub status: String,
    pub message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: ApiUser,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertProjectRequest {
    pub name: String,
    pub deploy_mode: String,
    pub repo_path: String,
    pub branch: String,
    pub workdir: Option<String>,
    pub build_command: String,
    pub built_path: String,
    /// 目标服务器；`local` 模式为 None。SSH 字段不再由项目维护。
    #[serde(default)]
    pub server_id: Option<i64>,
    pub deploy_path: String,
    pub stop_command: Option<String>,
    pub start_command: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListDeploymentsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookSecretRequest {
    pub webhook_secret: String,
}

#[derive(Debug, Deserialize)]
pub struct PushPayload {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub repository: Option<RepositoryPayload>,
}

#[derive(Debug, Deserialize)]
pub struct RepositoryPayload {
    pub full_name: String,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseWebhookPayload {
    pub action: Option<String>,
    pub repository: RepositoryPayload,
    pub release: ReleasePayload,
}

#[derive(Debug, Deserialize)]
pub struct ReleasePayload {
    pub tag_name: String,
    pub target_commitish: Option<String>,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArtifactReadyRequest {
    pub repository: String,
    pub branch: String,
    pub tag: Option<String>,
    pub commit: Option<String>,
    pub artifact_url: String,
    pub artifact_sha256: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct WebhookSecretResponse {
    pub webhook_secret: String,
}

#[derive(Debug, Serialize)]
pub struct DeploymentDetail {
    pub id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub branch: String,
    pub status: String,
    pub message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub log_text: Option<String>,
}
