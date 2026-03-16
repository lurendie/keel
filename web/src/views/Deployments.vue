<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  CircleCheck,
  Clock,
  Document,
  Refresh,
  Search,
  Warning,
} from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import StatusPill from "../components/StatusPill.vue";
import {
  getDeploymentDetail,
  listDeployments,
  type Deployment,
  type DeploymentDetail,
} from "../api/api";
import { formatTime } from "../utils/format";

const deployments = ref<Deployment[]>([]);
const selected = ref<DeploymentDetail | null>(null);
const loading = ref(true);
const detailLoading = ref(false);
const detailVisible = ref(false);
const searchText = ref("");
let refreshTimer: number | undefined;

const filteredDeployments = computed(() => {
  const keyword = searchText.value.trim().toLowerCase();
  if (!keyword) {
    return deployments.value;
  }

  return deployments.value.filter((deployment) =>
    [deployment.project_name, deployment.branch, deployment.status, deployment.message || ""]
      .join(" ")
      .toLowerCase()
      .includes(keyword),
  );
});

const loadDeployments = async (showMessage = false) => {
  loading.value = true;
  try {
    deployments.value = await listDeployments(100);
    if (showMessage) {
      ElMessage.success("部署记录已刷新");
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "部署记录加载失败");
  } finally {
    loading.value = false;
  }
};

const openDetail = async (deployment: Deployment) => {
  selected.value = null;
  detailVisible.value = true;
  detailLoading.value = true;

  try {
    selected.value = await getDeploymentDetail(deployment.id);
  } catch (error) {
    detailVisible.value = false;
    ElMessage.error(error instanceof Error ? error.message : "部署详情加载失败");
  } finally {
    detailLoading.value = false;
  }
};

const statusLabel = (status: string) => {
  if (status === "success") return "成功";
  if (status === "failed") return "失败";
  if (status === "running") return "执行中";
  if (status === "queued") return "排队中";
  return status;
};

const statusIcon = (status: string) => {
  if (status === "success") return CircleCheck;
  if (status === "failed") return Warning;
  return Clock;
};

const copyLog = async () => {
  if (!selected.value?.log_text) {
    ElMessage.info("当前没有可复制的日志");
    return;
  }

  try {
    await navigator.clipboard.writeText(selected.value.log_text);
    ElMessage.success("日志已复制");
  } catch {
    ElMessage.error("复制失败，请手动选择日志");
  }
};

onMounted(async () => {
  await loadDeployments();
  refreshTimer = window.setInterval(() => {
    if (!detailVisible.value) {
      loadDeployments();
    }
  }, 12000);
});

onUnmounted(() => {
  if (refreshTimer) {
    window.clearInterval(refreshTimer);
  }
});
</script>

<template>
    <div class="deployments-page">
        <PageIntro
            eyebrow="Workspace / Deployments"
            title="部署记录"
            description="每一次发布都有完整的状态、时间和执行日志。正在运行的任务会自动刷新。"
        >
            <template #actions>
                <el-button class="quiet-button" :loading="loading" @click="loadDeployments(true)">
                    <el-icon><Refresh /></el-icon>
                    刷新记录
                </el-button>
            </template>
        </PageIntro>

        <section class="deployments-content">
            <div class="deployments-toolbar">
                <div class="toolbar-copy">
                    <strong>{{ filteredDeployments.length }}</strong>
                    <span>条最近记录</span>
                    <span class="live-indicator"><i></i> 自动刷新</span>
                </div>
                <el-input
                    v-model="searchText"
                    class="search-input"
                    clearable
                    placeholder="搜索项目、分支或状态"
                >
                    <template #prefix><el-icon><Search /></el-icon></template>
                </el-input>
            </div>

            <div v-if="loading" class="state-panel">正在读取部署记录...</div>
            <div v-else-if="filteredDeployments.length === 0" class="empty-panel">
                <div class="empty-symbol"><Document /></div>
                <strong>{{ searchText ? "没有匹配的部署记录" : "还没有部署记录" }}</strong>
                <p>{{ searchText ? "换一个关键词试试。" : "项目第一次发布后，状态和日志会保存在这里。" }}</p>
            </div>
            <div v-else class="deployment-list">
                <button
                    v-for="deployment in filteredDeployments"
                    :key="deployment.id"
                    class="deployment-row"
                    type="button"
                    @click="openDetail(deployment)"
                >
                    <span class="deployment-number">#{{ String(deployment.id).padStart(4, "0") }}</span>
                    <span class="deployment-status-icon" :class="`status-${deployment.status}`">
                        <component :is="statusIcon(deployment.status)" />
                    </span>
                    <span class="deployment-summary">
                        <strong>{{ deployment.project_name }}</strong>
                        <span>{{ deployment.message || "部署任务" }}</span>
                    </span>
                    <span class="deployment-branch">{{ deployment.branch }}</span>
                    <StatusPill :status="deployment.status" :label="statusLabel(deployment.status)" />
                    <time>{{ formatTime(deployment.started_at) }}</time>
                </button>
            </div>
        </section>

        <el-drawer v-model="detailVisible" title="部署详情" size="min(720px, 94vw)" class="detail-drawer">
            <div v-if="detailLoading" class="drawer-state">正在读取完整日志...</div>
            <div v-else-if="selected" class="deployment-detail">
                <div class="detail-heading">
                    <div>
                        <span class="detail-eyebrow">Deployment #{{ String(selected.id).padStart(4, "0") }}</span>
                        <h2>{{ selected.project_name }}</h2>
                    </div>
                    <StatusPill :status="selected.status" :label="statusLabel(selected.status)" />
                </div>

                <div class="detail-meta">
                    <div><span>分支</span><strong>{{ selected.branch }}</strong></div>
                    <div><span>开始时间</span><strong>{{ formatTime(selected.started_at) }}</strong></div>
                    <div><span>结束时间</span><strong>{{ formatTime(selected.finished_at) }}</strong></div>
                </div>

                <div class="detail-message">
                    <span>状态消息</span>
                    <p>{{ selected.message || "没有额外消息" }}</p>
                </div>

                <div class="log-heading">
                    <span>Execution log</span>
                    <el-button text title="复制日志" @click="copyLog">
                        复制
                    </el-button>
                </div>
                <pre class="log-box">{{ selected.log_text || "暂无日志" }}</pre>
            </div>
        </el-drawer>
    </div>
</template>

<style scoped>
.deployments-page {
    min-height: 100%;
}

.deployments-content {
    padding: 28px 36px 44px;
}

.deployments-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
}

.toolbar-copy {
    display: flex;
    align-items: baseline;
    gap: 7px;
    color: #88928a;
    font-size: 12px;
}

.toolbar-copy strong {
    color: #354139;
    font-family: Georgia, serif;
    font-size: 24px;
    font-weight: 500;
}

.live-indicator {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: 10px;
    color: #4b5563;
    font-size: 11px;
}

.live-indicator i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #6b7280;
}

.quiet-button {
    color: #3d4b42;
    background: #f5f5f5;
    border: 1px solid #e2e2e2;
}

.quiet-button:hover {
    color: #202723;
    background: #ebebeb;
}

.search-input {
    width: min(300px, 48%);
}

.deployment-list,
.empty-panel,
.state-panel {
    background: #ffffff;
    border: 1px solid #d9ded9;
}

.deployment-row {
    display: grid;
    grid-template-columns: 72px 36px minmax(0, 1fr) 120px 84px 110px;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 75px;
    padding: 0 16px;
    color: inherit;
    text-align: left;
    background: transparent;
    border: 0;
    border-bottom: 1px solid #e1e5e1;
    cursor: pointer;
}

.deployment-row:last-child {
    border-bottom: 0;
}

.deployment-row:hover {
    background: #f5f5f5;
}

.deployment-number {
    color: #9ba39c;
    font-family: "Arial Narrow", "Segoe UI", sans-serif;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
}

.deployment-status-icon {
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    border-radius: 50%;
}

.deployment-status-icon :deep(svg) {
    width: 16px;
}

.status-success {
    color: #35688f;
    background: #eaf2f8;
}

.status-failed {
    color: #a94d42;
    background: #f8e6e1;
}

.status-queued,
.status-running {
    color: #8a6d2f;
    background: #f7efd5;
}

.deployment-summary {
    display: flex;
    min-width: 0;
    flex-direction: column;
}

.deployment-summary strong,
.deployment-summary span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.deployment-summary strong {
    color: #354139;
    font-size: 13px;
}

.deployment-summary span,
.deployment-branch,
.deployment-row time {
    margin-top: 4px;
    color: #8a948c;
    font-size: 11px;
}

.deployment-branch {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.deployment-row time {
    margin-top: 0;
    text-align: right;
}

.state-panel,
.empty-panel {
    display: flex;
    min-height: 250px;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: 10px;
    color: #778279;
    font-size: 13px;
}

.empty-panel strong {
    color: #354139;
    font-size: 15px;
}

.empty-panel p {
    margin: 0;
    color: #929b94;
    font-size: 12px;
}

.empty-symbol {
    display: grid;
    width: 42px;
    height: 42px;
    place-items: center;
    color: #525252;
    background: #f0f0f0;
}

.drawer-state {
    display: grid;
    min-height: 220px;
    place-items: center;
    color: #78837b;
    font-size: 13px;
}

.detail-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 22px;
    border-bottom: 1px solid #e1e5e1;
}

.detail-eyebrow {
    color: #879189;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.14em;
    text-transform: uppercase;
}

.detail-heading h2 {
    margin: 9px 0 0;
    color: #202723;
    font-family: Georgia, serif;
    font-size: 25px;
    font-weight: 500;
}

.detail-meta {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 14px;
    padding: 20px 0;
    border-bottom: 1px solid #e1e5e1;
}

.detail-meta div {
    display: flex;
    flex-direction: column;
    gap: 6px;
}

.detail-meta span,
.detail-message > span,
.log-heading span {
    color: #879189;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.1em;
    text-transform: uppercase;
}

.detail-meta strong {
    color: #354139;
    font-size: 12px;
    font-weight: 650;
}

.detail-message {
    padding: 20px 0;
}

.detail-message p {
    margin: 8px 0 0;
    color: #566359;
    font-size: 13px;
    line-height: 1.55;
}

.log-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 8px;
}

.log-box {
    min-height: 230px;
    max-height: 500px;
    margin: 0;
    padding: 16px;
    overflow: auto;
    color: #354139;
    background: #ffffff;
    border: 1px solid #dfe5e1;
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: 11px;
    line-height: 1.6;
    white-space: pre-wrap;
}

@media (max-width: 920px) {
    .deployments-content {
        padding: 24px;
    }

    .deployment-row {
        grid-template-columns: 60px 36px minmax(0, 1fr) 84px 100px;
    }

    .deployment-branch {
        display: none;
    }
}

@media (max-width: 720px) {
    .deployments-content {
        padding: 20px 18px 32px;
    }

    .deployments-toolbar {
        align-items: stretch;
        gap: 14px;
        flex-direction: column;
    }

    .search-input {
        width: 100%;
    }

    .deployment-row {
        grid-template-columns: 30px 36px minmax(0, 1fr) 82px;
        gap: 8px;
        min-height: 70px;
        padding: 0 10px;
    }

    .deployment-number {
        display: none;
    }

    .deployment-row time {
        display: none;
    }

    .deployment-row .status-pill {
        justify-self: end;
    }

    .detail-meta {
        grid-template-columns: 1fr;
    }
}
</style>
