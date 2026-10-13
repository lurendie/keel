<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ElMessage } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import MetricRing from "../components/MetricRing.vue";
import StatusChip from "../components/StatusChip.vue";
import {
  deployProject,
  getAllMetrics,
  listDeployments,
  listProjects,
  listServers,
  type Deployment,
  type Project,
  type Server,
  type ServerMetrics,
} from "../api/api";
import {
  displayStatus,
  statusLabel,
  versionFromMessage,
} from "../constants/deploy";
import { formatTime } from "../utils/format";

const router = useRouter();

const servers = ref<Server[]>([]);
const projects = ref<Project[]>([]);
const deployments = ref<Deployment[]>([]);
const metrics = ref<Map<number, ServerMetrics>>(new Map());
const sampledAt = ref("");
const loading = ref(true);
const sampling = ref(false);
const error = ref("");
const deployingId = ref<number | null>(null);

const enabledProjects = computed(() => projects.value.filter((p) => p.enabled));
const recentDeployments = computed(() => deployments.value.slice(0, 6));
const failedRecent = computed(() =>
  deployments.value.filter((d) => d.status === "failed"),
);

/** 只统计有指标且连得上的服务器，避免把离线机器算成「0% 很闲」。 */
const reachableCount = computed(
  () => servers.value.filter((s) => metrics.value.get(s.id)?.reachable).length,
);

/** 资源最吃紧的一项，作为「需要关注」的第二种信号。 */
const hotServer = computed(() => {
  const candidates: { server: Server; metric: string; value: number }[] = [];

  for (const server of servers.value) {
    const m = metrics.value.get(server.id);
    if (!m?.reachable) {
      continue;
    }
    const readings: [string, number | null][] = [
      ["CPU", m.cpu_percent],
      ["内存", m.mem_percent],
      ["磁盘", m.disk_percent],
    ];
    for (const [metric, value] of readings) {
      if (value !== null && value >= 85) {
        candidates.push({ server, metric, value: Math.round(value) });
      }
    }
  }

  return candidates.sort((a, b) => b.value - a.value)[0] ?? null;
});

function metricFor(serverId: number): ServerMetrics | undefined {
  return metrics.value.get(serverId);
}

function kbToGiB(kb: number | null): string {
  return kb === null ? "—" : `${(kb / 1024 / 1024).toFixed(1)} GiB`;
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const [serverList, projectList, deploymentList] = await Promise.all([
      listServers(),
      listProjects(),
      listDeployments(20),
    ]);
    servers.value = serverList;
    projects.value = projectList;
    deployments.value = deploymentList;
  } catch (err) {
    error.value = err instanceof Error ? err.message : "总览数据加载失败";
  } finally {
    loading.value = false;
  }
  // 采样要逐台建立 SSH 连接，比列表慢得多，单独跑，不挡住首屏。
  await sample();
}

async function sample() {
  if (servers.value.length === 0) {
    return;
  }
  sampling.value = true;
  try {
    const response = await getAllMetrics();
    const next = new Map<number, ServerMetrics>();
    for (const item of response.metrics) {
      next.set(item.server_id, item);
    }
    metrics.value = next;
    sampledAt.value = response.sampled_at;
  } catch (err) {
    ElMessage.warning(
      err instanceof Error ? `资源采样失败：${err.message}` : "资源采样失败",
    );
  } finally {
    sampling.value = false;
  }
}

async function refresh() {
  await load();
  if (!error.value) {
    ElMessage.success("已刷新");
  }
}

function projectFor(deployment: Deployment): Project | undefined {
  return projects.value.find((p) => p.id === deployment.project_id);
}

async function retry(deployment: Deployment) {
  const project = projectFor(deployment);
  if (!project) {
    ElMessage.warning("该项目已被删除");
    return;
  }
  deployingId.value = project.id;
  try {
    await deployProject(project.id);
    ElMessage.success(`已触发 ${project.name} 的部署`);
    await load();
  } catch (err) {
    ElMessage.error(err instanceof Error ? err.message : "触发部署失败");
  } finally {
    deployingId.value = null;
  }
}

function open(path: string) {
  void router.push(path);
}

let timer: number | undefined;

onMounted(async () => {
  await load();
  // 列表轮询保持轻量；资源采样只在刷新时重跑，避免频繁 SSH 登录。
  timer = window.setInterval(() => {
    if (document.visibilityState === "visible") {
      void load();
    }
  }, 30000);
});

onUnmounted(() => {
  if (timer) {
    window.clearInterval(timer);
  }
});
</script>

<template>
    <div>
        <PageIntro
            eyebrow="Overview"
            title="运行总览"
            description="服务器资源、项目数量与最近发布的实时状态。"
        >
            <template #actions>
                <span v-if="sampledAt" class="sampled mono">
                    采样于 {{ formatTime(sampledAt) }}
                </span>
                <el-button :loading="sampling" @click="refresh">刷新</el-button>
            </template>
        </PageIntro>

        <section class="content">
            <div v-if="error" class="error-bar">
                <span>{{ error }}</span>
                <el-button text @click="refresh">重试</el-button>
            </div>

            <div class="tiles">
                <div class="tile">
                    <div class="tile-v">
                        {{ servers.length }}
                        <i>/ {{ reachableCount }} 在线</i>
                    </div>
                    <div class="tile-k cap">服务器</div>
                </div>
                <div class="tile">
                    <div class="tile-v">
                        {{ enabledProjects.length }}
                        <i>/ {{ projects.length }} 启用</i>
                    </div>
                    <div class="tile-k cap">项目</div>
                </div>
                <div class="tile">
                    <div class="tile-v">
                        {{ deployments.length }}
                        <i>条最近记录</i>
                    </div>
                    <div class="tile-k cap">部署</div>
                </div>
                <div class="tile">
                    <div class="tile-v" :class="{ 'is-bad': failedRecent.length > 0 }">
                        {{ failedRecent.length }}
                        <i>次失败</i>
                    </div>
                    <div class="tile-k cap">需要关注</div>
                </div>
            </div>

            <div class="sec-head">
                <div>
                    <span class="cap">Server health</span>
                    <div class="h2">服务器资源</div>
                </div>
                <span class="dim">CPU / 内存 / 磁盘</span>
            </div>

            <div v-if="loading" class="state">正在读取服务器状态...</div>

            <div v-else-if="servers.length === 0" class="empty">
                <strong>还没有服务器</strong>
                <p>添加服务器后，这里会显示它们的资源占用。</p>
                <el-button type="primary" @click="open('/servers')">
                    去添加服务器
                </el-button>
            </div>

            <div v-else class="sv-grid">
                <article
                    v-for="server in servers"
                    :key="server.id"
                    class="sv"
                    :class="{ 'is-off': !metricFor(server.id)?.reachable }"
                >
                    <div class="sv-h">
                        <div class="sv-n">
                            <div class="sv-name">{{ server.name }}</div>
                            <div class="sv-addr">
                                {{ server.username }}@{{ server.host }}:{{ server.port }}
                            </div>
                        </div>
                        <StatusChip
                            :status="metricFor(server.id)?.reachable ? 'success' : 'failed'"
                            :label="metricFor(server.id)?.reachable ? '在线' : '采不到'"
                        />
                    </div>

                    <div class="sv-m">
                        <MetricRing
                            :value="metricFor(server.id)?.cpu_percent ?? null"
                            label="CPU"
                        />
                        <MetricRing
                            :value="metricFor(server.id)?.mem_percent ?? null"
                            label="内存"
                        />
                        <MetricRing
                            :value="metricFor(server.id)?.disk_percent ?? null"
                            label="磁盘"
                        />
                    </div>

                    <div class="sv-f">
                        <span class="sv-meta">
                            <span v-if="metricFor(server.id)?.os">
                                {{ metricFor(server.id)?.os }}
                            </span>
                            <span v-if="metricFor(server.id)?.reachable">
                                内存 {{ kbToGiB(metricFor(server.id)?.mem_used_kb ?? null) }}
                                / {{ kbToGiB(metricFor(server.id)?.mem_total_kb ?? null) }}
                                · {{ metricFor(server.id)?.disk_mount || "/" }}
                            </span>
                            <span v-else class="is-err">
                                {{ metricFor(server.id)?.error || "无法连接" }}
                            </span>
                        </span>
                        <el-button size="small" text @click="open('/servers')">
                            详情
                        </el-button>
                    </div>
                </article>
            </div>

            <template v-if="hotServer || failedRecent.length > 0">
                <div class="sec-head">
                    <span class="cap bad-cap">Needs attention</span>
                </div>

                <div v-if="hotServer" class="fault is-warn">
                    <div class="fault-body">
                        <div class="fault-n">
                            {{ hotServer.server.name }}
                            <StatusChip
                                status="running"
                                :label="`${hotServer.metric} ${hotServer.value}%`"
                            />
                        </div>
                        <div class="fault-m">资源占用偏高，建议检查进程或扩容。</div>
                    </div>
                </div>

                <div v-for="item in failedRecent.slice(0, 2)" :key="item.id" class="fault">
                    <div class="fault-body">
                        <div class="fault-n">
                            {{ item.project_name }}
                            <StatusChip status="failed" />
                        </div>
                        <div class="fault-e">{{ item.message || "部署失败" }}</div>
                        <div class="fault-m">
                            #{{ String(item.id).padStart(4, "0") }} · {{ item.branch }} ·
                            {{ formatTime(item.started_at) }}
                        </div>
                    </div>
                    <div class="fault-a">
                        <el-button
                            size="small"
                            :disabled="!projectFor(item)"
                            :loading="deployingId === item.project_id"
                            @click="retry(item)"
                        >
                            重试部署
                        </el-button>
                        <el-button size="small" @click="open('/deployments')">
                            查看日志
                        </el-button>
                    </div>
                </div>
            </template>

            <div class="sec-head">
                <div>
                    <span class="cap">Recent activity</span>
                    <div class="h2">最近发布</div>
                </div>
                <el-button text @click="open('/deployments')">全部记录 →</el-button>
            </div>

            <div v-if="!loading && recentDeployments.length === 0" class="empty">
                <strong>还没有部署记录</strong>
                <p>创建项目后手动触发一次部署，记录会出现在这里。</p>
                <el-button type="primary" @click="open('/projects')">
                    去创建项目
                </el-button>
            </div>

            <div v-else class="act">
                <div class="act-h cap">
                    <span>ID</span>
                    <span>项目</span>
                    <span>分支</span>
                    <span>版本</span>
                    <span>状态</span>
                    <span class="right">时间</span>
                </div>
                <button
                    v-for="item in recentDeployments"
                    :key="item.id"
                    class="act-r"
                    type="button"
                    @click="open('/deployments')"
                >
                    <span class="act-id">#{{ String(item.id).padStart(4, "0") }}</span>
                    <span class="act-p">{{ item.project_name }}</span>
                    <span class="act-m">{{ item.branch }}</span>
                    <span class="act-m">{{ versionFromMessage(item.message) || "—" }}</span>
                    <span>
                        <StatusChip
                            :status="displayStatus(item.status, item.message)"
                            :label="statusLabel(displayStatus(item.status, item.message))"
                        />
                    </span>
                    <span class="act-t">{{ formatTime(item.started_at) }}</span>
                </button>
            </div>
        </section>
    </div>
</template>

<style scoped>
.content { padding: 20px 24px 44px; }

.sampled { margin-right: 10px; color: var(--tx3); font-size: 11.5px; }

.error-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
    padding: 10px 13px;
    color: var(--bad);
    background: var(--bad-bg);
    border: 1px solid var(--bad-bd);
    border-radius: var(--r);
    font-size: 12.5px;
}

.tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }

.tile {
    padding: 13px 15px;
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
}

.tile-v {
    font-family: var(--font-mono);
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.02em;
    font-variant-numeric: tabular-nums;
}

.tile-v i { color: var(--tx3); font-size: 12px; font-style: normal; font-weight: 400; }
.tile-v.is-bad { color: var(--bad); }
.tile-k { margin-top: 1px; }

.sec-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    margin: 24px 0 0;
}

.sec-head .h2 { margin-top: 5px; }
.bad-cap { color: var(--bad); }

.state,
.empty {
    display: flex;
    min-height: 140px;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: 10px;
    margin-top: 10px;
    color: var(--tx3);
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
    font-size: 13px;
}

.empty strong { color: var(--tx); font-size: 14px; }
.empty p { margin: 0 0 4px; font-size: 12px; }

.sv-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(346px, 1fr));
    gap: 10px;
    margin-top: 10px;
}

.sv {
    display: flex;
    flex-direction: column;
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
    transition: border-color 120ms;
}

.sv:hover { border-color: var(--bd2); }
.sv.is-off { background: #fbfcfd; }

.sv-h { display: flex; align-items: flex-start; gap: 9px; padding: 12px 13px 0; }
.sv-n { min-width: 0; flex: 1; }
.sv-name {
    overflow: hidden;
    font-size: 13.5px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.sv-addr { margin-top: 2px; color: var(--tx3); font-family: var(--font-mono); font-size: 11px; }

.sv-m {
    display: flex;
    align-items: center;
    justify-content: space-around;
    gap: 8px;
    padding: 14px 13px 15px;
}

.sv-f {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 9px 13px;
    border-top: 1px solid var(--bd);
}

.sv-meta { display: flex; min-width: 0; flex-direction: column; gap: 1px; margin-right: auto; }
.sv-meta span {
    overflow: hidden;
    color: var(--tx3);
    font-family: var(--font-mono);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.sv-meta span.is-err { color: var(--bad); }

.fault {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-top: 10px;
    padding: 13px 14px;
    background: var(--sf);
    border: 1px solid var(--bad-bd);
    border-left: 3px solid var(--bad);
    border-radius: var(--r-lg);
}

.fault.is-warn { border-color: var(--run-bd); border-left-color: var(--run); }
.fault-body { min-width: 0; }
.fault-n { display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; }

.fault-e {
    margin-top: 7px;
    padding: 7px 10px;
    color: #8c2b20;
    background: var(--bad-bg);
    border: 1px solid var(--bad-bd);
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.6;
    word-break: break-all;
}

.fault-m { margin-top: 7px; color: var(--tx3); font-family: var(--font-mono); font-size: 11px; }
.fault-a { display: flex; flex-shrink: 0; gap: 7px; margin-left: auto; padding-left: 12px; }

.act {
    margin-top: 10px;
    overflow: hidden;
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
}

.act-h,
.act-r {
    display: grid;
    grid-template-columns: 58px minmax(130px, 1fr) 92px 80px 88px 84px;
    align-items: center;
    gap: 12px;
    padding: 0 13px;
}

.act-h { min-height: 31px; background: var(--sf2); border-bottom: 1px solid var(--bd); }
.act-h .right { text-align: right; }

.act-r {
    width: 100%;
    min-height: 40px;
    color: inherit;
    background: transparent;
    border: 0;
    border-bottom: 1px solid var(--bd);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    transition: background 120ms;
}

.act-r:last-child { border-bottom: 0; }
.act-r:hover { background: var(--sf2); }
.act-id { color: var(--tx4); font-family: var(--font-mono); font-size: 11px; }
.act-p {
    overflow: hidden;
    font-size: 12.5px;
    font-weight: 550;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.act-m {
    overflow: hidden;
    color: var(--tx2);
    font-family: var(--font-mono);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.act-t {
    color: var(--tx3);
    font-family: var(--font-mono);
    font-size: 11px;
    text-align: right;
    font-variant-numeric: tabular-nums;
}

@media (max-width: 1240px) { .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }

@media (max-width: 1000px) {
    .act-h,
    .act-r { grid-template-columns: 52px minmax(110px, 1fr) 86px 88px; }
    .act-h > :nth-child(3),
    .act-r > :nth-child(3),
    .act-h > :nth-child(4),
    .act-r > :nth-child(4) { display: none; }
}

@media (max-width: 760px) {
    .content { padding: 14px 12px 34px; }
    .tiles { grid-template-columns: 1fr; }
    .fault { flex-wrap: wrap; }
    .fault-a { width: 100%; margin-left: 0; padding: 9px 0 0; }
}
</style>
