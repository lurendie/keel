<script setup lang="ts">
import {
  Box,
  Clock,
  DataLine,
  MagicStick,
  Monitor,
  Setting,
} from "@element-plus/icons-vue";

const items = [
  { to: "/dashboard", label: "控制台", icon: DataLine },
  { to: "/servers", label: "服务器", icon: Monitor },
  { to: "/projects", label: "项目管理", icon: Box },
  { to: "/deployments", label: "部署记录", icon: Clock },
  { to: "/settings", label: "系统设置", icon: Setting },
  { to: "/ci-generator", label: "CI 生成器", icon: MagicStick },
];
</script>

<template>
    <aside class="layout-aside">
        <RouterLink class="brand" to="/dashboard" aria-label="Keel 控制台">
            <svg width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                <path d="M10 1.6v16.8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
                <path d="M10 3 2.6 6.6v4L10 7l7.4 3.6v-4z" fill="currentColor" />
                <path d="M4.2 12.4c1.3 2.9 3.5 4.6 5.8 5.4 2.3-.8 4.5-2.5 5.8-5.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
            </svg>
            <b>KEEL</b>
        </RouterLink>

        <nav class="nav" aria-label="主导航">
            <RouterLink
                v-for="item in items"
                :key="item.to"
                :to="item.to"
                class="nav-item"
            >
                <el-icon><component :is="item.icon" /></el-icon>
                <span>{{ item.label }}</span>
            </RouterLink>
        </nav>

        <div class="aside-foot">
            <span class="connection"><i />已连接</span>
        </div>
    </aside>
</template>

<style scoped>
.layout-aside {
    display: flex;
    width: var(--aside-w);
    min-width: var(--aside-w);
    min-height: 0;
    flex-direction: column;
    overflow: hidden;
    background: var(--sf);
    border-right: 1px solid var(--bd);
}

.brand {
    display: flex;
    height: var(--top-h);
    align-items: center;
    gap: 10px;
    padding: 0 14px;
    color: var(--tx);
    border-bottom: 1px solid var(--bd);
    flex-shrink: 0;
    text-decoration: none;
}

.brand svg { color: var(--ok); }
.brand b { font-size: 13px; font-weight: 700; letter-spacing: 0.16em; }

.nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 10px 8px;
}

.nav-item {
    display: flex;
    height: 34px;
    align-items: center;
    gap: 9px;
    padding: 0 9px;
    color: var(--tx2);
    border-radius: var(--r);
    font-size: 13px;
    text-decoration: none;
    transition: color 120ms, background 120ms;
}

.nav-item .el-icon { flex-shrink: 0; color: var(--tx3); font-size: 14px; }
.nav-item:hover { color: var(--tx); background: var(--sf2); }
.nav-item.router-link-active {
    color: var(--tx);
    background: var(--sf3);
    box-shadow: inset 2px 0 0 var(--ok);
    font-weight: 600;
}
.nav-item.router-link-active .el-icon { color: var(--tx2); }

.aside-foot {
    margin-top: auto;
    padding: 12px 14px 16px;
    border-top: 1px solid var(--bd);
}

.connection {
    display: inline-flex;
    height: 21px;
    align-items: center;
    gap: 6px;
    padding: 0 7px;
    color: var(--ok);
    background: var(--ok-bg);
    border: 1px solid var(--ok-bd);
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 11px;
}
.connection i { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }

@media (max-width: 760px) {
    .layout-aside {
        width: 100%;
        min-width: 0;
        height: 50px;
        min-height: 50px;
        flex-direction: row;
        align-items: center;
        overflow-x: auto;
        border-right: 0;
        border-bottom: 1px solid var(--bd);
    }

    .brand { height: 50px; padding: 0 12px; border-bottom: 0; }
    .nav { flex: 1; flex-direction: row; gap: 3px; padding: 0 10px 0 0; }
    .nav-item { height: 32px; flex-shrink: 0; white-space: nowrap; }
    .nav-item.router-link-active { box-shadow: inset 0 -2px 0 var(--ok); }
    .aside-foot { display: none; }
}
</style>
