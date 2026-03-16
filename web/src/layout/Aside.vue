<script setup lang="ts">
import { ref } from "vue";
import {
  Box,
  Expand,
  Fold,
  DataLine,
  Clock,
  MagicStick,
  Setting,
} from "@element-plus/icons-vue";

const isCollapse = ref(false);
</script>

<template>
    <el-aside :width="isCollapse ? '76px' : '236px'" class="layout-aside">
        <div class="aside-top">
            <div class="aside-mark">K</div>
            <div v-if="!isCollapse" class="aside-intro">
                <strong>Keel<br />Release Control</strong>
                <span>stable releases, clear signals</span>
            </div>
            <el-button
                class="collapse-button"
                text
                :aria-label="isCollapse ? '展开导航' : '收起导航'"
                @click="isCollapse = !isCollapse"
            >
                <el-icon>
                    <Expand v-if="isCollapse" />
                    <Fold v-else />
                </el-icon>
            </el-button>
        </div>

        <div class="nav-caption" :class="{ 'is-hidden': isCollapse }">Workspace</div>
        <el-menu
            :default-active="$route.path"
            :collapse="isCollapse"
            :collapse-transition="false"
            class="side-menu"
            router
        >
            <el-menu-item index="/dashboard">
                <el-icon><DataLine /></el-icon>
                <template #title>控制台</template>
            </el-menu-item>
            <el-menu-item index="/projects">
                <el-icon><Box /></el-icon>
                <template #title>项目管理</template>
            </el-menu-item>
            <el-menu-item index="/deployments">
                <el-icon><Clock /></el-icon>
                <template #title>部署记录</template>
            </el-menu-item>
            <el-menu-item index="/settings">
                <el-icon><Setting /></el-icon>
                <template #title>系统设置</template>
            </el-menu-item>
            <el-menu-item index="/ci-generator">
                <el-icon><MagicStick /></el-icon>
                <template #title>CI 生成器</template>
            </el-menu-item>
        </el-menu>

        <div v-if="!isCollapse" class="aside-note">
            <span class="note-dot"></span>
            <span>Ingress is listening</span>
        </div>
    </el-aside>
</template>

<style lang="css" scoped>
.layout-aside {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    overflow: hidden;
    background: #ffffff;
    border-right: 1px solid #e3e7e4;
    transition: width 0.2s ease;
}

.aside-top {
    display: flex;
    align-items: center;
    min-height: 92px;
    padding: 20px 16px 18px;
    border-bottom: 1px solid #e8ece9;
}

.aside-mark {
    display: grid;
    flex-shrink: 0;
    width: 42px;
    height: 42px;
    place-items: center;
    color: #202723;
    background: #f0f0f0;
    font-family: "Arial Narrow", "Segoe UI", sans-serif;
    font-size: 13px;
    font-weight: 800;
    letter-spacing: 0.1em;
}

.aside-intro {
    display: flex;
    min-width: 0;
    margin-left: 12px;
    flex-direction: column;
}

.aside-intro strong {
    color: #202723;
    font-size: 14px;
    line-height: 1.15;
    letter-spacing: 0.02em;
}

.aside-intro span {
    margin-top: 8px;
    color: #89928c;
    font-size: 10px;
    letter-spacing: 0.04em;
}

.collapse-button {
    margin-left: auto;
    color: #7b857e;
}

.collapse-button:hover,
.collapse-button:focus-visible {
    color: #171717;
    background: #f3f3f3;
}

.nav-caption {
    padding: 22px 24px 10px;
    color: #939c96;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    transition: opacity 0.2s ease;
}

.nav-caption.is-hidden {
    height: 42px;
    padding: 22px 0 0;
    opacity: 0;
}

.side-menu {
    flex: 1;
    border-right: 0;
    background: transparent;
}

.side-menu :deep(.el-menu-item) {
    height: 46px;
    margin: 3px 12px;
    border-radius: 4px;
    color: #5f6b64;
    font-size: 13px;
}

.side-menu :deep(.el-menu-item:hover) {
    color: #171717;
    background: #f5f5f5;
}

.side-menu :deep(.el-menu-item.is-active) {
    color: #171717;
    background: #eeeeee;
}

.side-menu :deep(.el-icon) {
    font-size: 17px;
}

.aside-note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 18px 20px 24px;
    padding-top: 14px;
    color: #748078;
    border-top: 1px solid #e8ece9;
    font-size: 11px;
}

.note-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #737373;
    box-shadow: 0 0 0 4px rgba(115, 115, 115, 0.12);
}

@media (max-width: 720px) {
    .layout-aside {
        position: fixed;
        z-index: 20;
        top: 0;
        bottom: 0;
        left: 0;
        width: 76px !important;
    }

    .aside-intro,
    .nav-caption,
    .aside-note {
        display: none;
    }

    .aside-top {
        justify-content: center;
        padding: 20px 12px;
    }

    .collapse-button {
        display: none;
    }

    .side-menu {
        width: 76px;
    }

    .side-menu :deep(.el-menu-item) {
        justify-content: center;
        margin: 3px 8px;
        padding: 0 !important;
        font-size: 0;
    }

    .side-menu :deep(.el-icon) {
        font-size: 17px;
    }
}
</style>
