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
    pub repo_path: String,
    pub branch: String,
    pub workdir: Option<String>,
    pub build_command: String,
    pub built_path: String,
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
    pub repo_path: String,
    pub branch: String,
    pub workdir: Option<String>,
    pub build_command: String,
    pub built_path: String,
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
