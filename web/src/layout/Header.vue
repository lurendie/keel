<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Bell, Connection, Refresh } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { useRouter } from "vue-router";
import { clearToken, me } from "../api/api";

const router = useRouter();
const username = ref("admin");
const isLoading = ref(false);

onMounted(async () => {
    try {
        const user = await me();
        username.value = user.username;
    } catch {
        clearToken();
        await router.replace("/login");
    }
});

const handleCommand = async (command: string) => {
    if (command === "logout") {
        clearToken();
        await router.replace("/login");
        return;
    }

    if (command === "settings") {
        await router.push("/settings");
    }
};

const refreshPage = () => {
    isLoading.value = true;
    window.setTimeout(() => window.location.reload(), 220);
};

const showConnectionInfo = () => {
    ElMessage({
        message: "Webhook ingress is available at /webhook",
        type: "info",
    });
};
</script>

<template>
    <el-header class="layout-header">
        <div class="header-context">
            <span class="context-kicker">KEEL / 2026</span>
            <span class="context-title">Release control</span>
        </div>

        <div class="header-right">
            <el-button class="header-tool" text aria-label="查看 webhook 状态" @click="showConnectionInfo">
                <el-icon><Connection /></el-icon>
                <span>Ingress</span>
            </el-button>
            <el-button
                class="header-tool"
                text
                aria-label="刷新页面"
                :loading="isLoading"
                @click="refreshPage"
            >
                <el-icon><Refresh /></el-icon>
            </el-button>
            <el-dropdown @command="handleCommand">
                <span class="user-info">
                    <span class="avatar">{{ username.slice(0, 1).toUpperCase() }}</span>
                    <span class="username">{{ username }}</span>
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

<style lang="css" scoped>
.layout-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 72px;
    padding: 0 28px;
    color: #202723;
    background: #ffffff;
    border-bottom: 1px solid #d9ded9;
}

.header-context {
    display: flex;
    flex-direction: column;
}

.context-kicker {
    color: #7c877f;
    font-family: "Arial Narrow", "Segoe UI", sans-serif;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.16em;
}

.context-title {
    margin-top: 5px;
    color: #202723;
    font-family: Georgia, serif;
    font-size: 21px;
    line-height: 1;
}

.header-right {
    display: flex;
    align-items: center;
    gap: 4px;
}

.header-tool {
    color: #657169;
    font-size: 12px;
}

.header-tool:hover,
.header-tool:focus-visible {
    color: #202723;
    background: #f2f2f2;
}

.user-info {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-left: 12px;
    padding-left: 16px;
    color: #202723;
    border-left: 1px solid #d9ded9;
    cursor: pointer;
}

.avatar {
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    color: #202723;
    background: #eeeeee;
    font-size: 12px;
    font-weight: 800;
}

.username {
    font-size: 12px;
    font-weight: 650;
}

@media (max-width: 720px) {
    .layout-header {
        height: 64px;
        padding: 0 16px 0 94px;
    }

    .context-title {
        overflow: hidden;
        font-size: 16px;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .header-tool span {
        display: none;
    }

    .user-info {
        margin-left: 4px;
        padding-left: 8px;
    }

    .username {
        display: none;
    }
}
</style>
