# Auto Deployment Server

Rust + Axum + SQLite 的自动部署后端，包含单用户登录系统、密码修改、项目管理、部署记录和 webhook 自动部署。

## 主要功能
- SQLite 统一存储（用户、项目、部署记录、系统设置）。
- 单用户系统（默认账号 `admin` / `admin`）。
- 登录、鉴权、修改密码接口。
- 项目管理接口（新增、更新、查询）。
- 手动触发部署 + webhook 自动触发部署。
- 部署记录查询（状态、时间、结果消息）。

## 运行
```bash
cargo run --release
```

## 环境变量
- `AUTO_DEPLOY_DB`：SQLite 文件路径，默认 `auto_deploy.db`。
- `JWT_SECRET`：JWT 签名密钥，生产环境必须修改。
- `GIT_PATH`：git 可执行文件路径，默认 `git`。

## 默认初始化
服务首次启动会自动建表并初始化：
- 用户：`admin` / `admin`
- settings：
  - `webhook_bind = 0.0.0.0:8000`
  - `webhook_path = /webhook`
  - `webhook_secret = ""`（空表示不校验签名）

## API 概览
- `POST /api/auth/login`
- `GET /api/auth/me`
- `POST /api/auth/change-password`
- `GET /api/projects`
- `POST /api/projects`
- `PUT /api/projects/:id`
- `DELETE /api/projects/:id`
- `POST /api/deploy/:project_id`
- `GET /api/deployments?limit=20`
- `GET /api/deployments/:id`（含完整部署日志）
- `GET /api/settings/webhook-secret`
- `PUT /api/settings/webhook-secret`
- `POST /webhook`（实际路径取 settings.webhook_path）
