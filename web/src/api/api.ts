export interface ApiUser {
  id: number;
  username: string;
  last_login_ip: string | null;
  last_login_at: string | null;
}

export type DeployMode = "local" | "ci_ssh";
export type AuthMethod = "key" | "password";

/**
 * SSH 连接信息集中维护在服务器上，项目只保存 server_id。
 * 凭据（密码、私钥口令）永远不会下发到前端，只回传 *_set 布尔标记。
 */
export interface Server {
  id: number;
  name: string;
  group_name: string | null;
  host: string;
  port: number;
  username: string;
  auth_method: AuthMethod;
  private_key_path: string | null;
  private_key_passphrase_set: boolean;
  password_set: boolean;
  deploy_path: string | null;
  enabled: boolean;
  /** 被多少个项目引用；删除前用来判断影响面。 */
  project_count: number;
  created_at: string;
  updated_at: string;
}

/** 更新时凭据字段为三态：不传保持原值、"" 清除、有值则覆盖。 */
export interface ServerPayload {
  name: string;
  group_name: string | null;
  host: string;
  port: number;
  username: string;
  auth_method: AuthMethod;
  private_key_path: string | null;
  private_key_passphrase?: string | null;
  password?: string | null;
  deploy_path: string | null;
  enabled: boolean;
}

export interface ServerTestResult {
  ok: boolean;
  message: string;
  detail: string;
}

export interface ServerMetrics {
  server_id: number;
  sampled_at: string;
  reachable: boolean;
  error: string | null;
  cpu_percent: number | null;
  load1: number | null;
  load5: number | null;
  load15: number | null;
  cpu_cores: number | null;
  mem_total_kb: number | null;
  mem_used_kb: number | null;
  mem_percent: number | null;
  swap_total_kb: number | null;
  swap_used_kb: number | null;
  disk_total_kb: number | null;
  disk_used_kb: number | null;
  disk_percent: number | null;
  disk_mount: string | null;
  uptime_seconds: number | null;
  os: string | null;
  kernel: string | null;
}

export interface ServerMetricsResponse {
  metrics: ServerMetrics[];
  sampled_at: string;
}

export interface Project {
  id: number;
  name: string;
  deploy_mode: DeployMode;
  repo_path: string;
  branch: string;
  workdir: string | null;
  build_command: string;
  built_path: string;
  server_id: number | null;
  server_name: string | null;
  deploy_path: string;
  stop_command: string | null;
  start_command: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export type ProjectPayload = Omit<
  Project,
  "id" | "server_name" | "created_at" | "updated_at"
>;

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

/**
 * 后端地址。
 *
 * 默认留空 → 请求发到当前页面所在的源。生产环境前端与后端是同一个服务
 * （Rust 直接托管静态文件），所以这是正确且最省事的做法：不管用 IP、域名
 * 还是反向代理访问，都不用改构建产物。
 *
 * 之前默认写死 `http://127.0.0.1:8000`，结果从别的机器打开页面时，浏览器
 * 会去打**访问者自己电脑**的 8000 端口，登录必然失败。
 *
 * 只有前后端分开部署（例如 vite dev server 连远程后端）时才需要设
 * VITE_API_BASE。
 */
const API_BASE = (import.meta.env.VITE_API_BASE as string | undefined) || "";
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

/* ------------------------------- 服务器 ------------------------------- */

export async function listServers(): Promise<Server[]> {
  return request("/api/servers");
}

export async function createServer(
  payload: ServerPayload,
): Promise<{ message: string }> {
  return request("/api/servers", {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export async function updateServer(
  id: number,
  payload: ServerPayload,
): Promise<{ message: string }> {
  return request(`/api/servers/${id}`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
}

export async function deleteServer(id: number): Promise<{ message: string }> {
  return request(`/api/servers/${id}`, { method: "DELETE" });
}

/** 连接测试始终返回 200，成功与否看 ok 字段。 */
export async function testServer(id: number): Promise<ServerTestResult> {
  return request(`/api/servers/${id}/test`, { method: "POST", body: "{}" });
}

export async function getServerMetrics(
  id: number,
): Promise<ServerMetricsResponse> {
  return request(`/api/servers/${id}/metrics`);
}

export async function getAllMetrics(): Promise<ServerMetricsResponse> {
  return request("/api/metrics");
}

/* ------------------------------- 项目 -------------------------------- */

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

/* ------------------------------ 部署记录 ------------------------------ */

export async function listDeployments(limit = 20): Promise<Deployment[]> {
  return request(`/api/deployments?limit=${limit}`);
}

export async function getDeploymentDetail(
  id: number,
): Promise<DeploymentDetail> {
  return request(`/api/deployments/${id}`);
}

/* ------------------------------ 系统设置 ------------------------------ */

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
