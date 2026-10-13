# Keel

Rust + Axum + SQLite 后端，配合 Vue 3 + Element Plus 管理界面，用于接收 GitHub / Gitea webhook，下载 CI/CD 制品，并通过 SSH 更新目标服务器。

## 主要能力
- 单用户登录、JWT 鉴权、修改密码。
- 项目管理：本机构建部署或 CI 制品 + SSH 部署。
- GitHub / Gitea push、Release webhook 和 CI 制品回调，支持 HMAC-SHA256 签名校验。
- 下载制品并校验 SHA-256。
- SSH 密钥登录、SFTP 上传、远程解压、停止/启动命令执行。
- 部署记录、状态、完整执行日志。
- 管理页面：控制台、项目管理、部署记录、系统设置。

## 运行后端
```bash
cd server
cargo run --release
```

默认监听 `0.0.0.0:8000`。

## 运行前端
```bash
cd web
npm install
npm run dev
```

默认开发地址为 `http://127.0.0.1:5173`。

## 默认初始化
- 用户名固定为 `admin`。
- 首次启动优先使用 `ADMIN_PASSWORD` 设置初始密码，密码至少 8 位。
- 未设置 `ADMIN_PASSWORD` 时，系统会生成随机密码并仅在首次启动日志中输出。
- 旧数据库如果仍在使用历史默认密码 `admin` 或 `admin123`，启动时会自动轮换。
- 登录成功后记录最近登录 IP 和时间，并在顶栏账号信息中展示。
- 连续 5 次登录失败后，同一来源 IP 会被锁定 5 分钟。
- `webhook_bind = 0.0.0.0:8000`
- `webhook_path = /webhook`
- `webhook_secret = ""`，此时 Push/Release webhook 不校验签名，CI 回调接口禁用

配置保存在 SQLite 中，不再读取 `config.toml`。项目配置请通过管理页面维护。

## 项目部署模式

项目保存 `deploy_mode`，管理页面会根据模式只显示对应字段：

- `local`：本机构建部署。
- `ci_ssh`：CI 制品 + SSH 部署。

### 本机构建
适用于部署服务所在机器可以直接访问仓库并执行构建的场景：

- `repo_path`：已有的本地仓库路径，或 `https://` / `ssh://` / `git@` 远程 Git 地址
- `branch`
- `workdir`：可选，构建命令的工作目录，相对于仓库根目录
- `build_command`
- `built_path`
- `deploy_path`：部署服务本机目标目录，必须是绝对路径，且不能是文件系统根目录

执行流程：
```text
本地路径：git fetch/pull -> build -> copy artifact -> deploy_path
远程地址：git clone/fetch -> build -> copy artifact -> deploy_path
```

远程 Git 地址会克隆到 `AUTO_DEPLOY_WORKSPACE` 管理的项目目录中。首次部署执行 clone，后续部署拉取更新。私有仓库需要提前为部署服务配置 Git 凭据、SSH Agent 或可用的 credential helper。

本机部署采用先写暂存再原子替换：新版本先完整拷贝到同级的 `.<目标名>.staging-<pid>`，再用 rename 切换到目标路径，因此目标目录不会出现写了一半的中间状态；上一版保留为 `.<目标名>.previous`，可用于手工回滚。

### CI 制品 + SSH
适用于 GitHub / Gitea Actions 等 CI 产出制品，再由本系统上传到远程服务器的场景：

- `repo_path`：GitHub/Gitea 的 `owner/repository`
- `ssh_host`、`ssh_port`、`ssh_user`
- `ssh_private_key_path`
- `ssh_private_key_passphrase`：可选
- `deploy_path`：远程服务器上的绝对路径，必须以 `/` 开头
- `stop_command`、`start_command`：可选

SSH 连接会校验目标主机公钥。首次使用前需要把目标主机公钥写入 `known_hosts`：

```bash
ssh-keyscan -p <ssh_port> <ssh_host> >> ~/.ssh/known_hosts
```

校验失败（公钥缺失或不匹配）会直接中止部署并在部署日志中说明原因。默认读取 `~/.ssh/known_hosts`，也可以用 `SSH_KNOWN_HOSTS` 指定其他文件。

执行流程：
```text
webhook / CI callback -> resolve artifact URL -> verify sha256 -> SSH upload
-> unzip/copy -> stop/start commands -> deployment log
```

支持的远程制品格式：
- `.zip`
- `.tar.gz` / `.tgz`
- 其他文件会直接复制到部署目录

制品下载限制：连接超时 15 秒，单次请求最长 5 分钟，制品体积上限 1 GiB。

## 环境变量
- `AUTO_DEPLOY_DB`：SQLite 文件路径，默认 `auto_deploy.db`。
- `ADMIN_PASSWORD`：首次初始化或轮换旧默认密码时使用的管理员密码。
- `JWT_SECRET`：JWT 签名密钥；未设置时会在 SQLite 中生成并持久化。
- `GIT_PATH`：git 可执行文件路径，默认 `git`。
- `AUTO_DEPLOY_WORKSPACE`：远程仓库和构建环境的受管目录，默认 `workspaces`。
- `DEPLOY_MAX_CONCURRENCY`：同时执行的部署任务上限，默认 `4`，允许范围 `1–32`。
- `LOG_DIR`：日志目录，默认 `logs`。
- `RUST_LOG`：控制台过滤规则，默认 `keel=info,tower_http=info`。
- `LOG_FORMAT`：控制台格式，`pretty` 或 `json`，默认 `pretty`。
- `SSH_KNOWN_HOSTS`：SSH 主机公钥文件路径，默认 `~/.ssh/known_hosts`。
- `TRUST_PROXY`：决定哪些对端可以下发 `CF-Connecting-IP`、`X-Real-IP`、`X-Forwarded-For`。
  - 不设置、`false`、`0`：只使用 TCP 对端地址，默认且最安全。
  - `true`：信任任意对端的转发头，仅在该端口无法被外部直接访问时使用。
  - `10.0.0.1,172.16.0.0/12`：只信任列表中的 IP 或 IPv4 网段。

启用 `TRUST_PROXY` 后请确认反向代理会覆盖写入上述请求头，否则客户端可以伪造登录 IP。

## 系统日志

服务使用 `tracing` 作为日志框架，并将每个级别写入独立文件。日志目录下只保留当天日志，历史日志压缩后按 `月日` 归档：

```text
logs/
├── keel.trace.log
├── keel.debug.log
├── keel.info.log
├── keel.warn.log
├── keel.error.log
└── 0912/
    ├── keel.info.20260912.log.gz
    └── keel.error.20260912.log.gz
```

日志策略：

- 单个文件超过 100MB 时滚动，当日文件继续写入 `logs/keel.<级别>.log`。
- 日期变化后，在下一条日志写入前把前一天的文件压缩归档到 `logs/<月日>/`。
- 进程停机跨天后重启时，会把遗留的旧日志先归档，保证根目录只有当天文件。
- 同一天多次滚动时按 `keel.<级别>.<日期>.<序号>.log.gz` 命名。
- 归档目录保留 7 天，过期目录整体删除。
- 控制台日志仍可通过 `RUST_LOG` 控制级别。

当前文件采用“级别及更高级别”的包含策略，例如 `info.log` 同时包含 `INFO`、`WARN` 和 `ERROR`；`error.log` 只包含错误。

## SQLite 容量
20–30 个项目对 SQLite 属于很小的数据量，项目、设置和部署记录的查询与写入不会成为瓶颈。当前系统已经启用 WAL，并为分支匹配和部署记录查询建立索引。

部署任务默认最多同时执行 4 个，不同项目可以并行构建、下载制品和连接 SSH；同一项目的任务会自动排队，避免重复发布互相覆盖。`DEPLOY_MAX_CONCURRENCY` 应根据 CPU、内存、网络和构建工具的实际负载调整。

SQLite 仍然不是这个规模下的瓶颈。只要部署频率是分钟级或更低，使用 SQLite 足够；即使扩展到数百个项目也通常可行。只有在同时运行多个服务进程、每秒持续写入大量部署记录，或需要跨机器共享任务队列时，才需要考虑 PostgreSQL/MySQL 和独立任务队列。

## 进程生命周期

- 收到 `SIGINT` / `SIGTERM` 后停止接收新请求，并最多等待 120 秒让正在执行的部署跑完。
- 启动时会把上次退出时仍处于 `queued` / `running` 的部署标记为失败，并追加“服务重启导致部署中断”，避免记录永远停在运行中。
- 当前设计面向单实例，同一份 SQLite 不要被多个进程同时启动。

## Webhook

统一接收路径：`POST /webhook`

GitHub：
- 事件头：`X-GitHub-Event: push`
- 签名头：`X-Hub-Signature-256: sha256=<hmac-sha256>`

Gitea：
- 事件头：`X-Gitea-Event: push`
- 签名头：`X-Gitea-Signature: <hmac-sha256-hex>`

系统按“仓库 + 分支”匹配已启用项目。项目的 `repo_path` 需要填写 `owner/repository` 形式，才能匹配 GitHub/Gitea 事件。

支持的事件：

- `push`：收到 push 后创建部署任务，适合本机构建或 CI 已同步完成制品的场景。
- `release` 且 action 为 `published`/`released`：从 Release assets 中选择制品，并覆盖本次部署地址。
- 自定义 CI 回调：GitHub Actions 编译完成后调用 `POST /api/deploy/callback`，动态传入精确制品地址。

Release asset 选择规则：

1. 如果 Release 只有一个 asset，则自动使用该 asset。
2. 如果有多个 asset，该 Release 不会触发部署；请使用 CI 制品回调传入精确地址。

### GitHub Actions 制品回调

调用接口：

```text
POST /api/deploy/callback
Header: X-Auto-Deploy-Signature: sha256=<HMAC-SHA256>
```

请求体：

```json
{
  "repository": "lurendie/auto_deployment",
  "branch": "main",
  "tag": "v1.2.3",
  "commit": "abc123",
  "artifact_url": "https://github.com/lurendie/auto_deployment/releases/download/v1.2.3/app.zip",
  "artifact_sha256": "64位SHA-256十六进制值",
  "message": "GitHub Actions build 123"
}
```

签名必须基于原始请求体计算。回调未配置 `webhook_secret` 时会直接拒绝。建议 CI 在上传 Release asset 成功后再调用，确保 URL 已可下载。

```bash
BODY='{"repository":"lurendie/auto_deployment","branch":"main","tag":"v1.2.3","artifact_url":"https://example.com/app.zip","artifact_sha256":"..."}'
SIGNATURE=$(printf '%s' "$BODY" | openssl dgst -sha256 -hmac "$WEBHOOK_SECRET" | awk '{print $2}')

curl -X POST "$DEPLOY_BASE_URL/api/deploy/callback" \
  -H "Content-Type: application/json" \
  -H "X-Auto-Deploy-Signature: sha256=$SIGNATURE" \
  --data "$BODY"
```

## API 概览
- `GET /health`（无需鉴权，数据库异常时返回 503）
- `POST /api/auth/login`
- `GET /api/auth/me`
- `POST /api/auth/change-password`
- `GET /api/projects`
- `POST /api/projects`
- `PUT /api/projects/:id`
- `DELETE /api/projects/:id`
- `POST /api/deploy/:project_id`
- `POST /api/deploy/callback`
- `GET /api/deployments?limit=20`
- `GET /api/deployments/:id`
- `GET /api/settings/webhook-secret`
- `PUT /api/settings/webhook-secret`
- `POST /webhook`

## 安全检查
- 默认 JWT Secret 仅用于开发环境。
- 生产环境必须配置强 `JWT_SECRET`。
- SSH 建议使用专用 deploy 用户和最小权限。
- 私钥路径只应在受信任服务器上配置，不要提交到仓库。
- `webhook_secret` 为空时 Push/Release webhook 不校验签名，启动时会输出告警，请务必配置。
- 修改密码后，此前签发的 JWT 会立即失效。
