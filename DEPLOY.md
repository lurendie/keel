# Keel 部署记录

同一套程序部署在两台腾讯云服务器上，用同一份 `docker-compose.yml`。

| | 服务器 A | 服务器 B |
|---|---|---|
| IP | `134.175.121.118` | `43.136.54.4` |
| 主机名 | `VM-0-12-debian` | `VM-0-2-debian` |
| 系统 | Debian 13 (trixie) x86_64 | Debian 13 (trixie) x86_64 |
| 规格 | 4 vCPU / 7.5 GiB / 118 GB | 4 vCPU / 3.7 GiB / 69 GB |
| 部署目录 | `/opt/keel` | `/opt/keel` |
| **访问地址** | **<http://134.175.121.118:8000>** | **<http://43.136.54.4:8000>** |
| 同机其他服务 | 1Panel（2233）、openresty（80/443）、PostgreSQL、Redis | 1Panel（2233）、**Gitea**（2222/30000）、NapCat、GS 核心 |

两台都是：容器 `keel`（`keel:latest`，278 MB）、`Up (healthy)`、重启策略
`unless-stopped`、监听 `0.0.0.0:8000`、网络 `keel_default` + `git_gitea-net`。

## ⚠️ 公网暴露的安全代价

端口 `8000` **直接对公网开放**，这是明确选饿\择后的结果，代价是：

1. **无 TLS** —— 登录口令与 JWT 明文过网，中间人可截获
2. **Keel 明文存储部署目标的 SSH 密码**（SQLite 内），一旦这台机器被拿下，
   所有被它部署的服务器凭据都会泄露
3. **任何人都能访问登录页**，只能靠鉴权挡住

已经生效的缓解措施：

- 登录失败限流**实测有效**：连续 6 次错误密码后返回 `429`
- `/webhook` 强制 HMAC-SHA256 校验，未签名请求返回 `401`
- 所有 `/api/*` 未带 token 一律 `401`

**`TRUST_PROXY` 必须保持 `false`。** 端口直接暴露时如果信任转发头，任何人都能
伪造 `X-Forwarded-For` 来绕过按 IP 的登录失败锁定，限流会形同虚设。

想收回内网时，把 `docker-compose.yml` 里的端口映射改回
`127.0.0.1:8000:8000`，再走下面的 1Panel 反代方案。

## GitHub Webhook 配置

服务器 B 的 Gitea 与 Keel 在同一 Docker 网络内，但 **GitHub 在公网**，
所以走公网地址：

```
Payload URL   http://43.136.54.4:8000/webhook
Content type  application/json
Secret        <该机器的 webhook-secret>
事件           Push、Releases
```

GitHub 侧要注意：

- 老的 `application/x-www-form-urlencoded` 已废弃，**必须选 `application/json`**，
  否则签名基于的报文与 Keel 解析的不一致
- GitHub 推荐 HTTPS。用 `http://` + 裸 IP 目前仍可提交，但若之后要求 HTTPS，
  就得按下面的一节配域名和证书

Keel 侧要注意：**项目的 `repo_path` 必须严格填 `owner/repository`**（例如
`lurendie/auto_deployment`），它按「仓库 + 分支」匹配项目。填成本地路径的
项目收不到 webhook。


## 登录凭据

管理员用户名固定 `admin`，密码是各机器独立随机生成的 32 位十六进制串：

```bash
# 服务器 A
grep KEEL_ADMIN_PASSWORD /opt/keel/.env     # 01018ea3788d13931f48967feb6aea70
# 服务器 B
grep KEEL_ADMIN_PASSWORD /opt/keel/.env     # 211860420d32ba8201da1ff44158182a
```

Webhook 签名密钥同样随机生成（64 位 hex）：

```bash
cat /opt/keel/webhook-secret.txt
# A: 3a0f1e9543514b8d8b5094b27fcc36316b0e812824f2edd2578298c8371f07b3
# B: 7be5d8913316eeac11df37a0439950635c012267be9fabe3b3a6ae19a07548a4
```

**请转存到密码管理器。** 部署时用明文口令登录过 root，建议改用密钥登录并
更换 root 密码。

## 怎么访问

两台都直接对公网开放 `8000`，浏览器打开即可：

- 服务器 A → <http://134.175.121.118:8000>
- 服务器 B → <http://43.136.54.4:8000>

域名与用户名见上面「登录凭据」一节。

### 想改成 HTTPS（可选，但推荐）

裸 IP + HTTP 的问题是登录口令明文过网。要上 TLS：

1. 一个**已备案的域名**解析到对应 IP（大陆服务器未备案的域名，80/443 会被运营商拦）
2. 1Panel → 网站 → 创建网站，填该域名
3. 反代目标 `http://127.0.0.1:8000`
4. 申请证书并开 HTTPS
5. 把 compose 里的端口映射改回 `127.0.0.1:8000:8000` 并 `docker compose up -d`

两台机器的 80/443 已被 1Panel 的 openresty 占用，不要去抢端口。

**只有走反代时**才可以打开 `TRUST_PROXY`（`true` 或写反代所在网段），
这样登录记录里才是真实客户端 IP。直连公网端口时打开它会让限流失效。

## 服务器 B 的 Gitea 联动

服务器 B 上跑着 Gitea，这是 Keel 天然的上游。两个容器已经接在同一个
Docker 网络 `git_gitea-net` 上，互通**不经过公网**，所以不受上面的端口封锁影响。

已验证：`docker exec keel curl http://gitea:30000/` 返回 200。

**配置 Gitea webhook**（Gitea → 仓库 → 设置 → Web 钩子 → 添加）：

```
目标 URL   http://keel:8000/webhook
HTTP 方法  POST
密钥       <服务器 B 的 webhook-secret>
触发事件   推送事件、发布事件
```

用容器名 `keel` 而不是 `43.136.54.4:8000`——容器名走 Docker 内部 DNS，
不经过公网，也不受运营商对端口的限制。现在公网地址虽然也通，但内网更稳。

**在 Keel 里把 Gitea 配成部署目标**时，如果部署服务也是这台机器上的容器，
仓库地址同样可以用 `http://gitea:30000/owner/repo.git` 而不是公网地址。

## 架构

```
浏览器 ─┐
        ├─ 127.0.0.1:8000 ─→ [ keel 容器 ]
Gitea ──┘   (git_gitea-net)      ├─ /            前端静态文件（Rust 直接托管）
                                 ├─ /api/*       JSON 接口（JWT）
                                 ├─ /webhook     Gitea 推送与 Release
                                 └─ /api/deploy/callback   CI 制品回调
```

前端产物由本地 `npm run build` 生成后打进镜像，容器里没有 Node——只为构建
前端再装一套 Node 会让镜像白白胖一圈。

数据库、日志、构建工作区都在具名卷里（`keel_keel-data` / `keel_keel-logs` /
`keel_keel-workspaces`），`docker compose down` 不丢数据，`down -v` 才会。

## 常用运维命令

```bash
cd /opt/keel

docker compose ps                    # 状态
docker compose logs -f --tail=100    # 日志
docker compose restart               # 重启
docker compose down                  # 停止（保留数据卷）
docker compose up -d --build         # 改完代码重新构建并启动
```

## 更新代码后怎么重新部署

两台机器上只有 Docker，没有 Rust/Node 工具链，所以流程是：

```bash
# 1. 本地构建前端
cd web && npm run build

# 2. 上传 server/ 与 web/dist/（排除 node_modules、target）
# 3. 服务器上重新构建
cd /opt/keel && docker compose up -d --build
```

**构建慢的坑**：`deb.debian.org` 和 `crates.io` 在这两台机器上都很慢（实测
apt 单步 12 分钟、cargo 索引 13 分钟无进展）。所以 `Dockerfile` 里 apt 源换成
腾讯云镜像，`server/cargo-config.toml` 把 crates 源换成 rsproxy。
换回官方源时删掉对应配置行即可。

## compose 里的外部网络

`docker-compose.yml` 引用了 `external: true` 的 `git_gitea-net`——它由 Gitea
那套 compose 创建，这里只加入不接管，避免 `docker compose down` 把它删掉
影响 Gitea。

服务器 A 上没有 Gitea，但为了两台共用一份配置，在 A 上建了个同名空网络占位：

```bash
docker network create git_gitea-net    # 只在新机器上需要
```

## 后端地址是怎么定的

前端**不写死后端地址**。`web/src/api/api.ts` 里：

```ts
const API_BASE = import.meta.env.VITE_API_BASE || "";
```

留空 → 所有请求发到**当前页面所在的源**（相对路径 `/api/...`）。
这样不管用 IP、域名还是反向代理访问，都不用改构建产物，也不会有跨域问题。

> 早期版本默认写死 `http://127.0.0.1:8000`，导致从别的机器打开页面时，浏览器
> 会去请求**访问者自己电脑**的 8000 端口，登录必然失败。
> 服务器 B 已修复；服务器 A 仍是旧产物（见文末「两台状态差异」）。

只有前后端分开部署时才需要设 `VITE_API_BASE`，例如本地 `vite dev` 连远程后端：

```bash
cd web
VITE_API_BASE=http://43.136.54.4:8000 npm run dev
```

## 还没做的事

**1. 没有 TLS。** 现在是裸 IP + HTTP，登录口令明文过网。见上面「想改成 HTTPS」。

**2. 服务器资源监控是空的。** 指标通过 SSH 采集，两台都还没添加目标服务器。
添加前先信任目标主机公钥：

```bash
ssh-keyscan -p <端口> <主机> >> /root/.ssh/known_hosts
```

容器以只读方式挂了宿主机的 `/root/.ssh`，所以宿主机上加的公钥容器能读到。

**3. 终端功能没有实现。** 界面上没有入口。真终端需要后端提供 WebSocket + PTY
（`request_pty` + `shell`）并做会话录制，是独立的一块工作。

**4. 两台实例各自独立。** 数据库不共享，项目与部署记录互不相通。如果希望
一处配置、两处执行，需要改成连同一个数据库，或者只在一台上跑 Keel 然后让它
通过 SSH 部署到另一台。

**5. ufw 拦不住 Docker 发布的端口。** 两台都开了 ufw，但 `ufw status` 里
并没有 8000 的放行规则，端口却照样通——因为 Docker 直接写 iptables 的
`DOCKER` 链，位置在 ufw 规则之前，等于绕过了 ufw。
要真正限制来源，得在腾讯云**安全组**里配，或在 `DOCKER-USER` 链里加规则。

## 两台状态差异

**服务器 B（43.136.54.4）** 是最新的：前端已改为相对路径请求，浏览器直接
打开 `http://43.136.54.4:8000` 就能正常登录使用。

**服务器 A（134.175.121.118）** 仍停留在修复前的镜像，产物里写死了
`http://127.0.0.1:8000`。**从别的机器打开它的页面，登录会失败**——浏览器会去
请求访问者本机的 8000 端口。按使用者要求暂不处理。

将来要修服务器 A，按下面三步（源码已修好，只需重新构建）：

```bash
cd web && npm run build                      # 本地出新产物
# 上传 web/dist/ 到服务器 A 的 /opt/keel/web/dist
cd /opt/keel && docker compose up -d --build # 重建镜像
```
