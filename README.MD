# Auto Deployment

一个轻量的自动部署服务（Rust + Axum）。通过 GitHub Webhook 触发部署流程，拉取最新代码，执行构建步骤，将产物复制到部署目录，并可选启停服务与邮件通知。

**功能概览**
- 接收 GitHub `push` Webhook 并校验签名（可选）。
- 按分支过滤，只对指定分支触发部署。
- 支持多项目、多步骤部署。
- 支持文件或目录部署。
- 串行化部署，避免并发冲突。
- 记录日志并清理 7 天前日志（可配置目录）。
- 发送成功/失败邮件通知（可选）。

**运行方式**
1. 复制 `config.example.toml` 为 `config.toml` 并按需修改。
1. 启动服务：

```bash
cargo run --release
```

**配置说明**
配置文件为 TOML，默认读取 `config.toml`。也可通过环境变量指定：
- `AUTO_DEPLOY_CONFIG`：配置文件路径。
- `AUTO_DEPLOY_LOG_DIR`：日志目录（默认 `logs`）。

关键字段说明：
- `webhook_bind`：监听地址，默认 `0.0.0.0:8000`
- `webhook_path`：Webhook 路径，默认 `/webhook`
- `webhook_secret`：GitHub Webhook Secret（可选）
- `git_path`：Git 可执行文件路径（可选，默认 `git`）
- `email`：SMTP 邮件通知配置（可选）
- `projects`：项目列表
- `projects.branches`：允许的分支列表（为空或不配置表示全部允许）
- `projects.steps`：部署步骤列表（必填）

示例配置（精简）：

```toml
webhook_bind = "0.0.0.0:8000"
webhook_path = "/webhook"
webhook_secret = "your_secret"

[email]
smtp_server = "smtp.gmail.com"
smtp_port = 587
smtp_username = "your@gmail.com"
smtp_password = "your_app_password"
from = "your@gmail.com"
to = ["your@gmail.com"]
subject_prefix = "[AutoDeploy]"

[[projects]]
name = "project-a"
repo_path = "E:/path/to/project-a"
branches = ["main"]

  [[projects.steps]]
  name = "build"
  workdir = "backend"
  build_command = "cargo build --release"
  built_path = "backend/target/release/project-a.exe"
  deploy_path = "E:/path/to/run/project-a.exe"
  stop_command = "taskkill /F /IM project-a.exe"
  start_command = "E:/path/to/run/project-a.exe"
```

**部署流程**
1. 服务器收到 `push` Webhook。
1. 校验签名（如果配置了 `webhook_secret`）。
1. 判断是否为目标分支。
1. `git fetch` + 比对 `HEAD` 与 `origin/<branch>`。
1. 有更新则 `git pull --ff-only`。
1. 执行配置的步骤：
- 在指定 `workdir` 执行 `build_command`。
- 需要时执行 `stop_command`。
- 将 `built_path` 复制到 `deploy_path`（支持目录）。
- 需要时执行 `start_command`。

**并发处理**
所有部署任务串行执行。新推送到达时会排队，待前一个部署完成后继续执行。部署前会重新比对远端提交，因此不会丢更新，但可能将多次推送合并为一次部署。

**日志**
- 控制台输出 + 文件日志（默认 `logs/auto_deploy-YYYY-MM-DD.log`）。
- 启动时自动清理 7 天前的日志文件。

**安全提示**
- Webhook Secret 建议启用，以防伪造请求。
- 生产环境建议限制 `webhook_bind` 和上游网络访问范围。

