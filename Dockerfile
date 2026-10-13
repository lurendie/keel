# syntax=docker/dockerfile:1

# ---------------------------------------------------------------
# Keel 后端。多阶段构建：先编译 Rust 二进制，再放进最小运行镜像。
# 前端产物由本地/CI 的 `npm run build` 生成后放在 web/dist，这里只拷贝——
# 容器里为了构建前端再装一套 Node 只会白白拉大镜像。
#
# 国内网络注意：deb.debian.org 与 crates.io 在这台机器上都很慢
# （实测 apt 单步 12 分钟、cargo 索引 13 分钟无进展），所以换成腾讯云
# 与 rsproxy 镜像。要回到官方源删掉对应的 mirror 行即可。
# ---------------------------------------------------------------
FROM rust:1-bookworm AS builder

# CARGO_HOME 是 rust 官方镜像里 cargo 的安装位置，配置放这里才会被读取
ENV CARGO_HOME=/usr/local/cargo
COPY server/cargo-config.toml $CARGO_HOME/config.toml

WORKDIR /build
COPY server/ ./
RUN cargo build --release

# ---------------------------------------------------------------
FROM debian:bookworm-slim

RUN set -eux; \
    if [ -f /etc/apt/sources.list.d/debian.sources ]; then \
        sed -i 's|deb.debian.org|mirrors.tencent.com|g' /etc/apt/sources.list.d/debian.sources; \
    fi; \
    if [ -f /etc/apt/sources.list ]; then \
        sed -i 's|deb.debian.org|mirrors.tencent.com|g' /etc/apt/sources.list; \
    fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        openssh-client \
        git \
        curl; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/keel /usr/local/bin/keel
COPY web/dist /app/web

# 数据库、日志、构建工作区都落在挂载卷上，容器重建不丢数据
RUN mkdir -p /data /logs /workspaces

ENV KEEL_WEB_ROOT=/app/web \
    AUTO_DEPLOY_DB=/data/auto_deploy.db \
    AUTO_DEPLOY_WORKSPACE=/workspaces \
    LOG_DIR=/logs \
    LOG_FORMAT=json \
    RUST_LOG=keel=info,tower_http=info

EXPOSE 8000

# 数据库异常时 /health 返回 503，正好可以作为存活探针
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8000/health || exit 1

ENTRYPOINT ["/usr/local/bin/keel"]
