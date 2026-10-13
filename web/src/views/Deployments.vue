<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { ElMessage } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import StatusChip from "../components/StatusChip.vue";
import DeployRail from "../components/DeployRail.vue";
import LogConsole from "../components/LogConsole.vue";
import {
  getDeploymentDetail,
  listDeployments,
  listProjects,
  type Deployment,
  type DeploymentDetail,
  type DeployMode,
  type Project,
} from "../api/api";
import {
  displayStatus,
  failedStageFromLog,
  parseLog,
  runningStageFromLog,
  statusLabel,
  versionFromMessage,
} from "../constants/deploy";
import { formatTime } from "../utils/format";
import { useRoute, useRouter } from "vue-router";

const route = useRoute();
const router = useRouter();

const deployments = ref<Deployment[]>([]);
const projects = ref<Project[]>([]);
const selected = ref<DeploymentDetail | null>(null);
const selectedId = ref<number | null>(null);
const loading = ref(true);
const detailLoading = ref(false);
const searchText = ref("");
const statusFilter = ref<"all" | "failed" | "running" | "queued">("all");
let refreshTimer: number | undefined;

const filtered = computed(() => {
  const keyword = searchText.value.trim().toLowerCase();
  return deployments.value.filter((d) => {
    if (statusFilter.value !== "all" && d.status !== statusFilter.value) {
      return false;
    }
    if (!keyword) {
      return true;
    }
    return [d.project_name, d.branch, d.status, d.message || ""]
      .join(" ")
      .toLowerCase()
      .includes(keyword);
  });
});

const counts = computed(() => ({
  all: deployments.value.length,
  failed: deployments.value.filter((d) => d.status === "failed").length,
  running: deployments.value.filter((d) => d.status === "running").length,
  queued: deployments.value.filter((d) => d.status === "queued").length,
}));

/** 部署方式决定阶段序列，需要从项目里查。 */
function modeFor(deployment: Deployment): DeployMode {
  return projects.value.find((p) => p.id === deployment.project_id)?.deploy_mode ?? "ci_ssh";
}

const detailLines = computed(() => parseLog(selected.value?.log_text));

/**
 * 失败停在哪个阶段：后端不记录阶段边界，只能从日志里的命令条数推断。
 * 推断不出来时返回 null，轨道保持 idle 而不是编造进度。
 */
const failedStage = computed(() =>
  selected.value?.status === "failed" ? failedStageFromLog(detailLines.value) : null,
);

const nowStage = computed(() =>
  selected.value?.status === "running" ? runningStageFromLog(detailLines.value) : null,
);

const errorDetail = computed(() => {
  if (selected.value?.status !== "failed") {
    return null;
  }
  const errors = detailLines.value.filter((l) => l.kind === "err");
  if (errors.length > 0) {
    return errors.map((l) => l.text).join("\n");
  }
  return selected.value.message;
});

async function load(showMessage = false) {
  loading.value = true;
  try {
    const [deploymentList, projectList] = await Promise.all([
      listDeployments(100),
      listProjects(),
    ]);
    deployments.value = deploymentList;
    projects.value = projectList;
    if (showMessage) {
      ElMessage.success("部署记录已刷新");
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "部署记录加载失败");
  } finally {
    loading.value = false;
  }
}

async function openDetail(deployment: Deployment) {
  selectedId.value = deployment.id;
  selected.value = null;
  detailLoading.value = true;
  // 深链接：刷新或分享后仍停在同一条记录
  void router.replace({ query: { ...route.query, d: String(deployment.id) } });

  try {
    selected.value = await getDeploymentDetail(deployment.id);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "部署详情加载失败");
  } finally {
    detailLoading.value = false;
  }
}

onMounted(async () => {
  await load();
  // 支持 #/deployments?d=142 直接打开某条记录
  const wanted = Number(route.query.d);
  const target = Number.isFinite(wanted)
    ? deployments.value.find((d) => d.id === wanted)
    : deployments.value[0];
  if (target) {
    await openDetail(target);
  }

  refreshTimer = window.setInterval(() => {
    if (document.visibilityState === "visible") {
      void load();
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
    <div>
        <PageIntro
            eyebrow="Workspace / Deployments"
            title="部署记录"
            description="每条记录都有完整状态与执行日志。运行中的任务会定时刷新。"
        >
            <template #actions>
                <el-button :loading="loading" @click="load(true)">刷新记录</el-button>
            </template>
        </PageIntro>

        <section class="content">
            <div class="filters">
                <el-radio-group v-model="statusFilter" size="small">
                    <el-radio-button value="all">全部 {{ counts.all }}</el-radio-button>
                    <el-radio-button value="failed">失败 {{ counts.failed }}</el-radio-button>
                    <el-radio-button value="running">执行中 {{ counts.running }}</el-radio-button>
                    <el-radio-button value="queued">排队 {{ counts.queued }}</el-radio-button>
                </el-radio-group>
                <el-input
                    v-model="searchText"
                    class="search"
                    clearable
                    placeholder="搜索项目、分支、版本"
                />
            </div>

            <div class="split">
                <div class="list">
                    <div v-if="loading" class="state">正在读取部署记录...</div>
                    <template v-else>
                        <button
                            v-for="item in filtered"
                            :key="item.id"
                            class="item"
                            :class="{ 'is-active': item.id === selectedId }"
                            type="button"
                            @click="openDetail(item)"
                        >
                            <span class="item-id">#{{ String(item.id).padStart(4, "0") }}</span>
                            <span class="item-p">{{ item.project_name }}</span>
                            <StatusChip
                                :status="displayStatus(item.status, item.message)"
                                :label="statusLabel(displayStatus(item.status, item.message))"
                            />
                            <span class="item-s">
                                {{ item.branch }}
                                <template v-if="versionFromMessage(item.message)">
                                    · {{ versionFromMessage(item.message) }}
                                </template>
                            </span>
                        </button>
                    </template>

                    <div v-if="!loading && filtered.length === 0" class="state">
                        {{ searchText ? "没有匹配的部署记录" : "还没有部署记录" }}
                    </div>
                </div>

                <div class="detail-col">
                    <div v-if="detailLoading" class="panel detail">
                        <div class="state">正在读取完整日志...</div>
                    </div>

                    <div v-else-if="selected" class="panel detail">
                        <div class="detail-h">
                            <div>
                                <span class="cap">
                                    Deployment #{{ String(selected.id).padStart(4, "0") }}
                                </span>
                                <div class="detail-n">{{ selected.project_name }}</div>
                            </div>
                            <StatusChip
                                :status="displayStatus(selected.status, selected.message)"
                                :label="statusLabel(displayStatus(selected.status, selected.message))"
                            />
                        </div>

                        <dl class="dmeta">
                            <div>
                                <dt>分支</dt>
                                <dd>{{ selected.branch }}</dd>
                            </div>
                            <div>
                                <dt>版本</dt>
                                <dd>{{ versionFromMessage(selected.message) || "—" }}</dd>
                            </div>
                            <div>
                                <dt>开始</dt>
                                <dd>{{ formatTime(selected.started_at) }}</dd>
                            </div>
                            <div>
                                <dt>结束</dt>
                                <dd>{{ formatTime(selected.finished_at) }}</dd>
                            </div>
                        </dl>

                        <div class="dsec">
                            <div class="dsec-t">
                                <span class="cap">Pipeline</span>
                                <span class="cap">
                                    {{ modeFor(selected) === "ci_ssh" ? "CI 制品 → SSH" : "本机构建" }}
                                </span>
                            </div>
                            <DeployRail
                                :status="selected.status"
                                :mode="modeFor(selected)"
                                :now-stage="nowStage"
                                :failed-stage="failedStage"
                                :halted="selected.status === 'queued' && !selected.log_text"
                                :error-detail="errorDetail"
                            />
                        </div>

                        <div class="dsec">
                            <div v-if="selected.log_text" class="dsec-t">
                                <span class="cap">Log</span>
                                <span class="cap">
                                    {{ detailLines.length }} 行
                                </span>
                            </div>
                            <LogConsole
                                v-if="selected.log_text"
                                :log-text="selected.log_text"
                            />
                            <div v-else class="note">
                                本次任务没有落库日志。
                                <template v-if="selected.status === 'running'">
                                    运行中的执行输出只在任务结束后写入数据库，上面的轨道反映的是真实进度。
                                </template>
                            </div>
                        </div>
                    </div>

                    <div v-else class="panel detail">
                        <div class="state">从左侧选择一条部署记录</div>
                    </div>
                </div>
            </div>
        </section>
    </div>
</template>

<style scoped>
.content { padding: 20px 24px 44px; }

.filters { display: flex; align-items: center; gap: 10px; }
.search { width: 240px; margin-left: auto; }

.split {
    display: grid;
    grid-template-columns: 340px minmax(0, 1fr);
    gap: 12px;
    margin-top: 12px;
    align-items: start;
}

.detail-col { align-self: stretch; min-width: 0; }

.list {
    overflow: hidden;
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
}

.item {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr) auto;
    align-items: center;
    gap: 3px 9px;
    width: 100%;
    padding: 9px 11px;
    color: inherit;
    background: transparent;
    border: 0;
    border-bottom: 1px solid var(--bd);
    border-left: 2px solid transparent;
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    transition: background 120ms;
}

.item:last-child { border-bottom: 0; }
.item:hover { background: var(--sf2); }
.item.is-active { background: var(--tag-bg); border-left-color: var(--tag); }
.item-id { color: var(--tx4); font-family: var(--font-mono); font-size: 11px; }
.item-p {
    overflow: hidden;
    font-size: 12.5px;
    font-weight: 550;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.item-s {
    grid-column: 2 / 4;
    overflow: hidden;
    color: var(--tx3);
    font-family: var(--font-mono);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.state {
    display: flex;
    min-height: 120px;
    align-items: center;
    justify-content: center;
    padding: 16px;
    color: var(--tx3);
    font-size: 12.5px;
}

.detail { position: sticky; top: 0; }
.detail-h {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 14px;
    padding: 13px 15px;
    border-bottom: 1px solid var(--bd);
}
.detail-n { margin-top: 4px; font-size: 16px; font-weight: 650; letter-spacing: -0.01em; }

.dmeta {
    display: flex;
    flex-wrap: wrap;
    margin: 0;
    padding: 11px 15px;
    border-bottom: 1px solid var(--bd);
}
.dmeta > div { padding-right: 20px; }
.dmeta > div + div { padding-left: 20px; border-left: 1px solid var(--bd); }
.dmeta dt {
    color: var(--tx4);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
}
.dmeta dd {
    margin: 3px 0 0;
    font-family: var(--font-mono);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
}

.dsec { border-bottom: 1px solid var(--bd); }
.dsec:last-child { border-bottom: 0; }
.dsec-t {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 11px 15px 0;
}

.note {
    margin: 12px 15px;
    padding: 9px 11px;
    color: var(--tx3);
    background: var(--sf2);
    border: 1px solid var(--bd);
    border-radius: var(--r);
    font-size: 12px;
    line-height: 1.6;
}

@media (max-width: 1000px) {
    .split { grid-template-columns: minmax(0, 1fr); }
    .detail-col { align-self: auto; }
    .detail { position: static; }
}

@media (max-width: 760px) {
    .content { padding: 14px 12px 34px; }
    .filters { flex-wrap: wrap; }
    .search { width: 100%; margin-left: 0; }
}
</style>
