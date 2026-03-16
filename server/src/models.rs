use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiUser {
    pub id: i64,
    pub username: String,
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
    pub artifact_url: Option<String>,
    pub artifact_sha256: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<i64>,
    pub ssh_user: Option<String>,
    pub ssh_private_key_path: Option<String>,
    #[serde(skip_serializing)]
    pub ssh_private_key_passphrase: Option<String>,
    pub ssh_private_key_passphrase_set: bool,
    pub deploy_path: String,
    pub stop_command: Option<String>,
    pub start_command: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
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
    pub artifact_url: Option<String>,
    pub artifact_sha256: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<i64>,
    pub ssh_user: Option<String>,
    pub ssh_private_key_path: Option<String>,
    pub ssh_private_key_passphrase: Option<String>,
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
