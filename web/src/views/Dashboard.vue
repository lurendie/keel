<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ArrowRight, Box, CircleCheck, Clock, Plus, Refresh } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import StatusPill from "../components/StatusPill.vue";
import {
  listDeployments,
  listProjects,
  type Deployment,
  type Project,
} from "../api/api";
import {  formatTime } from "../utils/format";

const router = useRouter();
const projects = ref<Project[]>([]);
const deployments = ref<Deployment[]>([]);
const loading = ref(true);
const error = ref("");

const enabledProjects = computed(() => projects.value.filter((project) => project.enabled));
const successfulDeployments = computed(
  () => deployments.value.filter((deployment) => deployment.status === "success").length,
);
const failedDeployments = computed(
  () => deployments.value.filter((deployment) => deployment.status === "failed").length,
);

const loadDashboard = async () => {
  loading.value = true;
  error.value = "";

  try {
    const [projectList, deploymentList] = await Promise.all([
      listProjects(),
      listDeployments(8),
    ]);
    projects.value = projectList;
    deployments.value = deploymentList;
  } catch (err) {
    error.value = err instanceof Error ? err.message : "控制台数据加载失败";
  } finally {
    loading.value = false;
  }
};

const open = (path: string) => router.push(path);

const refresh = async () => {
  await loadDashboard();
  if (!error.value) {
    ElMessage.success("控制台已刷新");
  }
};

onMounted(loadDashboard);
</script>

<template>
    <div class="dashboard-page">
        <div v-if="error" class="dashboard-error">
            <span>{{ error }}</span>
            <el-button text @click="refresh">重试</el-button>
        </div>

        <section class="dashboard-content">


            <div class="metric-grid">
                <div class="metric-card">
                    <span class="metric-label">可部署项目</span>
                    <strong>{{ enabledProjects.length }}</strong>
                    <span class="metric-foot">共 {{ projects.length }} 个项目</span>
                </div>
                <div class="metric-card">
                    <span class="metric-label">最近成功</span>
                    <strong>{{ successfulDeployments }}</strong>
                    <span class="metric-foot">最近 8 条记录</span>
                </div>
                <div class="metric-card" :class="{ 'is-alert': failedDeployments > 0 }">
                    <span class="metric-label">需要关注</span>
                    <strong>{{ failedDeployments }}</strong>
                    <span class="metric-foot">失败任务</span>
                </div>
                <div class="metric-card metric-action" @click="open('/projects')">
                    <span class="metric-label">下一步</span>
                    <strong>管理项目 <ArrowRight /></strong>
                    <span class="metric-foot">配置制品源与 SSH 目标</span>
                </div>
            </div>

            <div class="section-heading">
                <div>
                    <span class="section-kicker">Latest activity</span>
                    <h3>最近部署</h3>
                </div>
                <el-button text class="text-button" @click="open('/deployments')">
                    查看全部 <ArrowRight />
                </el-button>
            </div>

            <div v-if="loading" class="state-panel">正在读取部署状态...</div>
            <div v-else-if="deployments.length === 0" class="empty-panel">
                <div class="empty-icon"><Box /></div>
                <strong>还没有部署记录</strong>
                <p>创建第一个项目后，手动触发一次部署，记录会出现在这里。</p>
                <el-button type="primary" @click="open('/projects')">
                    <el-icon><Plus /></el-icon>
                    创建项目
                </el-button>
            </div>
            <div v-else class="activity-list">
                <div v-for="deployment in deployments.slice(0, 5)" :key="deployment.id" class="activity-row">
                    <div class="activity-icon" :class="`status-${deployment.status}`">
                        <CircleCheck v-if="deployment.status === 'success'" />
                        <Clock v-else />
                    </div>
                    <div class="activity-main">
                        <strong>{{ deployment.project_name }}</strong>
                        <span>{{ deployment.message || `${deployment.branch} 分支部署任务` }}</span>
                    </div>
                    <div class="activity-branch">{{ deployment.branch }}</div>
                    <StatusPill :status="deployment.status" :label="deployment.status === 'success' ? '成功' : deployment.status === 'failed' ? '失败' : '进行中'" />
                    <time>{{ formatTime(deployment.started_at) }}</time>
                </div>
            </div>
        </section>
    </div>
</template>

<style scoped>
.dashboard-page {
    min-height: 100%;
}

.dashboard-content {
    padding: 28px 36px 44px;
}

.dashboard-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 20px 36px 0;
    padding: 12px 14px;
    color: #9b4c42;
    background: #f8e6e1;
    border: 1px solid #edc7bd;
    font-size: 13px;
}

.quiet-button {
    color: #000000;
    background: #ffffff;
    border: 1px solid #ffffff;
}

.quiet-button:hover {
    color: #202723;
    background: #ffffff;
}

.signal-strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 190px;
    padding: 28px 32px;
    overflow: hidden;
    background: #ffffff;
    color: #202723;
    border: 1px solid #ffffff;
}

.signal-copy {
    position: relative;
    z-index: 1;
    max-width: 520px;
}

.signal-kicker,
.section-kicker {
    color: #737373;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.16em;
    text-transform: uppercase;
}

.signal-copy h2 {
    margin: 10px 0 8px;
    font-family: Georgia, serif;
    font-size: 30px;
    font-weight: 500;
    letter-spacing: 0;
}

.signal-copy p {
    max-width: 490px;
    margin: 0;
    color: #6e786f;
    font-size: 13px;
    line-height: 1.55;
}

.signal-visual {
    position: relative;
    width: min(36vw, 330px);
    height: 112px;
    margin-right: 10px;
}

.signal-line {
    position: absolute;
    top: 55px;
    right: 0;
    left: 0;
    height: 1px;
    background: #ffffff;
}

.signal-node,
.signal-core {
    position: absolute;
    display: grid;
    place-items: center;
    border-radius: 50%;
}

.signal-node {
    top: 50px;
    width: 11px;
    height: 11px;
    background: #525252;
    box-shadow: 0 0 0 6px rgba(82, 82, 82, 0.1);
}

.node-one {
    left: 18%;
}

.node-two {
    left: 48%;
}

.node-three {
    right: 12%;
}

.signal-core {
    top: 25px;
    left: 39%;
    width: 62px;
    height: 62px;
    color: #262626;
    background: #ffffff;
    border: 1px solid #ffffff;
    box-shadow: 0 0 0 10px rgba(82, 82, 82, 0.05);
}

.signal-core :deep(svg) {
    width: 23px;
    height: 23px;
}

.signal-visual.is-active .signal-core {
    animation: pulse 1.8s ease-in-out infinite;
}

.metric-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 10px;
    margin-top: 10px;
}

.metric-card {
    display: flex;
    min-height: 118px;
    padding: 18px 20px;
    flex-direction: column;
    background: #ffffff;
    border: 1px solid #d9ded9;
}

.metric-label {
    color: #78837b;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
}

.metric-card strong {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 13px;
    color: #202723;
    font-family: Georgia, serif;
    font-size: 29px;
    font-weight: 500;
}

.metric-card strong :deep(svg) {
    width: 19px;
}

.metric-foot {
    margin-top: auto;
    color: #929b94;
    font-size: 11px;
}

.metric-card.is-alert strong {
    color: #a94d42;
}

.metric-action {
    cursor: pointer;
}

.metric-action:hover {
    background: #ffffff;
}

.metric-action strong {
    font-family: inherit;
    font-size: 17px;
    font-weight: 700;
}

.section-heading {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    margin: 34px 0 14px;
}

.section-heading h3 {
    margin: 7px 0 0;
    color: #202723;
    font-family: Georgia, serif;
    font-size: 23px;
    font-weight: 500;
}

.text-button {
    color: #58735f;
    font-size: 12px;
}

.state-panel,
.empty-panel {
    display: flex;
    min-height: 160px;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    color: #768178;
    background: #ffffff;
    border: 1px solid #ffffff;
    font-size: 13px;
}

.empty-panel {
    gap: 10px;
}

.empty-panel strong {
    color: #354139;
    font-size: 15px;
}

.empty-panel p {
    margin: 0 0 6px;
    color: #8a938c;
    font-size: 12px;
}

.empty-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    color: #525252;
    background: #f0f0f0;
}

.activity-list {
    background: #ffffff;
    border: 1px solid #d9ded9;
}

.activity-row {
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) 120px 86px 110px;
    align-items: center;
    gap: 12px;
    min-height: 66px;
    padding: 0 16px;
    border-bottom: 1px solid #e1e5e1;
}

.activity-row:last-child {
    border-bottom: 0;
}

.activity-icon {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border-radius: 50%;
}

.activity-icon :deep(svg) {
    width: 15px;
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

.activity-main {
    display: flex;
    min-width: 0;
    flex-direction: column;
}

.activity-main strong {
    overflow: hidden;
    color: #354139;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.activity-main span,
.activity-branch,
.activity-row time {
    overflow: hidden;
    color: #879189;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.activity-row time {
    text-align: right;
}

@keyframes pulse {
    0%,
    100% {
        box-shadow: 0 0 0 10px rgba(82, 82, 82, 0.07);
    }
    50% {
        box-shadow: 0 0 0 20px rgba(82, 82, 82, 0.03);
    }
}

@media (max-width: 920px) {
    .metric-grid {
        grid-template-columns: repeat(2, 1fr);
    }

    .activity-row {
        grid-template-columns: 34px minmax(0, 1fr) 86px 96px;
    }

    .activity-branch {
        display: none;
    }
}

@media (max-width: 720px) {
    .dashboard-content {
        padding: 20px 18px 34px;
    }

    .signal-strip {
        min-height: 230px;
        padding: 24px;
    }

    .signal-visual {
        position: absolute;
        right: 22px;
        bottom: 24px;
        width: 180px;
        height: 70px;
        opacity: 0.55;
        transform: scale(0.8);
        transform-origin: bottom right;
    }

    .signal-copy h2 {
        max-width: 230px;
        font-size: 26px;
    }

    .activity-row {
        grid-template-columns: 34px minmax(0, 1fr) 82px;
        gap: 8px;
        padding: 0 10px;
    }

    .activity-row .status-pill {
        justify-self: end;
    }

    .activity-row time {
        display: none;
    }
}

@media (prefers-reduced-motion: reduce) {
    .signal-visual.is-active .signal-core {
        animation: none;
    }
}
</style>
