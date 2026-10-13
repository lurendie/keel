<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Connection, Refresh } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { useRoute, useRouter } from "vue-router";
import { clearToken, me } from "../api/api";

const route = useRoute();
const router = useRouter();
const username = ref("admin");
const lastLoginIp = ref<string | null>(null);
const lastLoginAt = ref<string | null>(null);
const isLoading = ref(false);

const titles: Record<string, string> = {
  "/dashboard": "运行总览",
  "/servers": "服务器",
  "/projects": "项目管理",
  "/deployments": "部署记录",
  "/settings": "系统设置",
  "/ci-generator": "CI 生成器",
};

const pageTitle = computed(() => titles[route.path] || "Keel");
const loginSummary = computed(() => {
  const time = lastLoginAt.value ? new Date(lastLoginAt.value) : null;
  const formatted = time && !Number.isNaN(time.getTime()) ? time.toLocaleString() : lastLoginAt.value;
  return [lastLoginIp.value, formatted].filter(Boolean).join(" · ");
});

onMounted(async () => {
  try {
    const user = await me();
    username.value = user.username;
    lastLoginIp.value = user.last_login_ip;
    lastLoginAt.value = user.last_login_at;
  } catch {
    clearToken();
    await router.replace("/login");
  }
});

async function handleCommand(command: string) {
  if (command === "logout") {
    clearToken();
    await router.replace("/login");
  } else if (command === "settings") {
    await router.push("/settings");
  }
}

function refreshPage() {
  isLoading.value = true;
  window.setTimeout(() => window.location.reload(), 180);
}

function showConnectionInfo() {
  ElMessage({ message: "Webhook 接收端点：/webhook", type: "info" });
}
</script>

<template>
    <el-header class="layout-header">
        <span class="page-title">{{ pageTitle }}</span>

        <div class="header-right">
            <el-button class="header-tool" text aria-label="查看 Webhook 状态" @click="showConnectionInfo">
                <el-icon><Connection /></el-icon>
                <span>Webhook</span>
            </el-button>
            <el-button class="header-tool icon-button" text aria-label="刷新页面" :loading="isLoading" @click="refreshPage">
                <el-icon><Refresh /></el-icon>
            </el-button>
            <el-dropdown @command="handleCommand">
                <span class="user-info">
                    <span class="avatar">{{ username.slice(0, 1).toUpperCase() }}</span>
                    <span class="user-summary">
                        <span class="username">{{ username }}</span>
                        <span v-if="loginSummary" class="login-origin" :title="loginSummary">{{ loginSummary }}</span>
                    </span>
                </span>
                <template #dropdown>
                    <el-dropdown-menu>
                        <el-dropdown-item command="settings">设置</el-dropdown-item>
                        <el-dropdown-item divided command="logout">退出登录</el-dropdown-item>
                    </el-dropdown-menu>
                </template>
            </el-dropdown>
        </div>
    </el-header>
</template>

<style scoped>
.layout-header {
    display: flex;
    height: var(--top-h);
    min-height: var(--top-h);
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 18px;
    color: var(--tx);
    background: var(--sf);
    border-bottom: 1px solid var(--bd);
}

.page-title { overflow: hidden; font-size: 13.5px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
.header-right { display: flex; align-items: center; gap: 3px; }
.header-tool { height: 29px; color: var(--tx2); font-size: 12px; }
.header-tool:hover,
.header-tool:focus-visible { color: var(--tx); background: var(--sf2); }
.icon-button { width: 29px; padding: 0; }

.user-info {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: 7px;
    padding-left: 10px;
    color: var(--tx);
    border-left: 1px solid var(--bd);
    cursor: pointer;
}
.avatar {
    display: grid;
    width: 26px;
    height: 26px;
    place-items: center;
    color: var(--ok);
    background: var(--ok-bg);
    border: 1px solid var(--ok-bd);
    border-radius: var(--r);
    font-size: 11px;
    font-weight: 700;
}
.user-summary { display: flex; min-width: 0; flex-direction: column; gap: 1px; }
.username { font-size: 12px; font-weight: 600; }
.login-origin {
    max-width: 260px;
    overflow: hidden;
    color: var(--tx4);
    font-size: 9.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

@media (max-width: 760px) {
    .layout-header { height: 48px; min-height: 48px; padding: 0 12px; }
    .header-tool span,
    .user-summary { display: none; }
    .user-info { margin-left: 2px; padding-left: 7px; }
}
</style>
