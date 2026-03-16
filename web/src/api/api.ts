export interface ApiUser {
  id: number;
  username: string;
}

export type DeployMode = "local" | "ci_ssh";

export interface Project {
  id: number;
  name: string;
  deploy_mode: DeployMode;
  repo_path: string;
  branch: string;
  workdir: string | null;
  build_command: string;
  built_path: string;
  artifact_url: string | null;
  artifact_sha256: string | null;
  ssh_host: string | null;
  ssh_port: number | null;
  ssh_user: string | null;
  ssh_private_key_path: string | null;
  ssh_private_key_passphrase_set: boolean;
  deploy_path: string;
  stop_command: string | null;
  start_command: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export type ProjectPayload = Omit<
  Project,
  | "id"
  | "ssh_private_key_passphrase_set"
  | "created_at"
  | "updated_at"
> & {
  ssh_private_key_passphrase: string | null;
};

export interface Deployment {
  id: number;
  project_id: number;
  project_name: string;
  branch: string;
  status: string;
  message: string | null;
  started_at: string;
  finished_at: string | null;
}

export interface DeploymentDetail extends Deployment {
  log_text: string | null;
}

export interface WebhookSecretResponse {
  webhook_secret: string;
}

const API_BASE =
  (import.meta.env.VITE_API_BASE as string | undefined) ||
  "http://127.0.0.1:8000";
const TOKEN_KEY = "auto_deploy_token";

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) || "";
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers || {});
  headers.set("Content-Type", "application/json");

  const token = getToken();
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers,
  });

  if (!response.ok) {
    const text = await response.text();
    let message = text;
    try {
      const json = JSON.parse(text) as { error?: string };
      message = json.error || text;
    } catch {
      // keep raw text
    }
    throw new Error(message || `HTTP ${response.status}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return (await response.json()) as T;
}

export async function login(
  username: string,
  password: string,
): Promise<{ token: string; user: ApiUser }> {
  return request("/api/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password }),
  });
}

export async function me(): Promise<ApiUser> {
  return request("/api/auth/me");
}

export async function changePassword(
  old_password: string,
  new_password: string,
): Promise<{ message: string }> {
  return request("/api/auth/change-password", {
    method: "POST",
    body: JSON.stringify({ old_password, new_password }),
  });
}

export async function listProjects(): Promise<Project[]> {
  return request("/api/projects");
}

export async function createProject(
  payload: ProjectPayload,
): Promise<{ message: string }> {
  return request("/api/projects", {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export async function updateProject(
  id: number,
  payload: ProjectPayload,
): Promise<{ message: string }> {
  return request(`/api/projects/${id}`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
}

export async function deleteProject(id: number): Promise<{ message: string }> {
  return request(`/api/projects/${id}`, {
    method: "DELETE",
  });
}

export async function deployProject(
  projectId: number,
): Promise<{ message: string }> {
  return request(`/api/deploy/${projectId}`, {
    method: "POST",
    body: "{}",
  });
}

export async function listDeployments(limit = 20): Promise<Deployment[]> {
  return request(`/api/deployments?limit=${limit}`);
}

export async function getDeploymentDetail(
  id: number,
): Promise<DeploymentDetail> {
  return request(`/api/deployments/${id}`);
}

export async function getWebhookSecret(): Promise<WebhookSecretResponse> {
  return request("/api/settings/webhook-secret");
}

export async function updateWebhookSecret(
  webhook_secret: string,
): Promise<{ message: string }> {
  return request("/api/settings/webhook-secret", {
    method: "PUT",
    body: JSON.stringify({ webhook_secret }),
  });
}
