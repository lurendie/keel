<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ElMessage, ElMessageBox, type FormInstance, type FormRules } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import StatusChip from "../components/StatusChip.vue";
import {
  createProject,
  deleteProject,
  deployProject,
  listProjects,
  listServers,
  updateProject,
  type Project,
  type ProjectPayload,
  type Server,
} from "../api/api";
const projects = ref<Project[]>([]);
const servers = ref<Server[]>([]);
const loading = ref(true);
const saving = ref(false);
const dialogVisible = ref(false);
const dialogMode = ref<"create" | "edit">("create");
const searchText = ref("");
const editingId = ref<number | null>(null);
const formRef = ref<FormInstance>();

const emptyProject = (): ProjectPayload => ({
  name: "",
  deploy_mode: "ci_ssh",
  repo_path: "",
  branch: "main",
  workdir: null,
  build_command: "",
  built_path: "",
  // SSH 连接信息不再由项目维护，这里只保存指向服务器的引用
  server_id: null,
  deploy_path: "",
  stop_command: null,
  start_command: null,
  enabled: true,
});

const form = reactive<ProjectPayload>(emptyProject());

const rules: FormRules<ProjectPayload> = {
  name: [{ required: true, message: "请输入项目名称", trigger: "blur" }],
  repo_path: [
    { required: true, message: "请输入仓库标识或本地仓库路径", trigger: "blur" },
  ],
  branch: [{ required: true, message: "请输入部署分支", trigger: "blur" }],
  build_command: [
    {
      validator: (_rule, value, callback) => {
        if (form.deploy_mode === "local" && !String(value || "").trim()) {
          callback(new Error("请输入本机构建命令"));
          return;
        }
        callback();
      },
      trigger: "blur",
    },
  ],
  built_path: [
    {
      validator: (_rule, value, callback) => {
        if (form.deploy_mode === "local" && !String(value || "").trim()) {
          callback(new Error("请输入本地产物路径"));
          return;
        }
        callback();
      },
      trigger: "blur",
    },
  ],
  server_id: [
    {
      validator: (_rule, value, callback) => {
        if (form.deploy_mode === "ci_ssh" && (value === null || value === undefined)) {
          callback(new Error("请选择目标服务器"));
          return;
        }
        callback();
      },
      trigger: "change",
    },
  ],
  deploy_path: [{ required: true, message: "请输入部署目录", trigger: "blur" }],
};

const isRemoteMode = computed(() => form.deploy_mode === "ci_ssh");

const filteredProjects = computed(() => {
  const keyword = searchText.value.trim().toLowerCase();
  if (!keyword) {
    return projects.value;
  }

  return projects.value.filter((project) =>
    [
      project.name,
      project.repo_path,
      project.branch,
      project.server_name || "",
    ]
      .join(" ")
      .toLowerCase()
      .includes(keyword),
  );
});

/** 只列出启用的服务器；编辑时若项目指向已停用的服务器也一并保留，避免选择被清空。 */
const selectableServers = computed(() =>
  servers.value.filter((s) => s.enabled || s.id === form.server_id),
);

function serverName(project: Project): string | null {
  if (project.server_name) {
    return project.server_name;
  }
  const found = servers.value.find((s) => s.id === project.server_id);
  return found ? found.name : null;
}

async function load() {
  loading.value = true;
  try {
    const [projectList, serverList] = await Promise.all([
      listProjects(),
      listServers(),
    ]);
    projects.value = projectList;
    servers.value = serverList;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "项目加载失败");
  } finally {
    loading.value = false;
  }
}

function resetForm() {
  Object.assign(form, emptyProject());
  editingId.value = null;
}

function openCreate() {
  dialogMode.value = "create";
  resetForm();
  dialogVisible.value = true;
}

function openEdit(project: Project) {
  dialogMode.value = "edit";
  editingId.value = project.id;
  Object.assign(form, {
    name: project.name,
    deploy_mode: project.deploy_mode,
    repo_path: project.repo_path,
    branch: project.branch,
    workdir: project.workdir,
    build_command: project.build_command,
    built_path: project.built_path,
    server_id: project.server_id,
    deploy_path: project.deploy_path,
    stop_command: project.stop_command,
    start_command: project.start_command,
    enabled: project.enabled,
  });
  dialogVisible.value = true;
}

async function saveProject() {
  const valid = await formRef.value?.validate().catch(() => false);
  if (!valid) {
    return;
  }

  saving.value = true;
  try {
    const remoteMode = form.deploy_mode === "ci_ssh";
    const payload: ProjectPayload = {
      ...form,
      repo_path: form.repo_path.trim(),
      build_command: remoteMode ? "" : form.build_command.trim(),
      built_path: remoteMode ? "" : form.built_path.trim(),
      server_id: remoteMode ? form.server_id : null,
      workdir: remoteMode ? null : form.workdir?.trim() || null,
      stop_command: form.stop_command?.trim() || null,
      start_command: form.start_command?.trim() || null,
    };

    if (dialogMode.value === "create") {
      await createProject(payload);
      ElMessage.success("项目已创建");
    } else if (editingId.value !== null) {
      await updateProject(editingId.value, payload);
      ElMessage.success("项目已更新");
    }

    dialogVisible.value = false;
    await load();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "保存失败");
  } finally {
    saving.value = false;
  }
}

async function removeProject(project: Project) {
  try {
    await ElMessageBox.confirm(
      `删除「${project.name}」会同时移除它的部署记录，是否继续？`,
      "删除项目",
      { type: "warning", confirmButtonText: "删除", cancelButtonText: "取消" },
    );
    await deleteProject(project.id);
    ElMessage.success("项目已删除");
    await load();
  } catch (error) {
    if (error !== "cancel" && error !== "close") {
      ElMessage.error(error instanceof Error ? error.message : "删除项目失败");
    }
  }
}

async function runDeploy(project: Project) {
  try {
    await ElMessageBox.confirm(
      `立即为「${project.name}」创建一次部署任务？`,
      "手动部署",
      { confirmButtonText: "开始部署", cancelButtonText: "取消" },
    );
    await deployProject(project.id);
    ElMessage.success("部署任务已创建");
  } catch (error) {
    if (error !== "cancel" && error !== "close") {
      ElMessage.error(error instanceof Error ? error.message : "创建部署任务失败");
    }
  }
}

async function toggleEnabled(project: Project) {
  try {
    await updateProject(project.id, {
      name: project.name,
      deploy_mode: project.deploy_mode,
      repo_path: project.repo_path,
      branch: project.branch,
      workdir: project.workdir,
      build_command: project.build_command,
      built_path: project.built_path,
      server_id: project.server_id,
      deploy_path: project.deploy_path,
      stop_command: project.stop_command,
      start_command: project.start_command,
      enabled: !project.enabled,
    });
    await load();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "状态更新失败");
  }
}

onMounted(load);
</script>

<template>
    <div>
        <PageIntro
            eyebrow="Workspace / Projects"
            title="项目管理"
            description="项目只关心「构建什么、发到哪台服务器的哪个目录」；SSH 连接信息在服务器页统一维护。"
        >
            <template #actions>
                <el-input
                    v-model="searchText"
                    class="search"
                    clearable
                    placeholder="搜索项目、分支或服务器"
                />
                <el-button type="primary" @click="openCreate">新建项目</el-button>
            </template>
        </PageIntro>

        <section class="content">
            <div v-if="loading" class="state">正在读取项目配置...</div>

            <div v-else-if="filteredProjects.length === 0" class="empty">
                <strong>{{ searchText ? "没有匹配的项目" : "还没有项目" }}</strong>
                <p>
                    {{
                        searchText
                            ? "换一个关键词试试。"
                            : "创建项目后，就可以接入 CI 制品和服务器。"
                    }}
                </p>
                <el-button v-if="!searchText" type="primary" @click="openCreate">
                    创建第一个项目
                </el-button>
            </div>

            <div v-else class="grid">
                <article v-for="project in filteredProjects" :key="project.id" class="card">
                    <div class="card-h">
                        <span class="card-n">{{ project.name }}</span>
                        <StatusChip
                            :status="project.enabled ? 'success' : 'queued'"
                            :label="project.enabled ? '已启用' : '已停用'"
                        />
                        <el-dropdown trigger="click">
                            <el-button text size="small">⋯</el-button>
                            <template #dropdown>
                                <el-dropdown-menu>
                                    <el-dropdown-item @click="openEdit(project)">
                                        编辑
                                    </el-dropdown-item>
                                    <el-dropdown-item @click="toggleEnabled(project)">
                                        {{ project.enabled ? "停用" : "启用" }}
                                    </el-dropdown-item>
                                    <el-dropdown-item divided @click="removeProject(project)">
                                        删除
                                    </el-dropdown-item>
                                </el-dropdown-menu>
                            </template>
                        </el-dropdown>
                    </div>

                    <div class="card-kind">
                        <span class="cap">
                            {{
                                project.deploy_mode === "ci_ssh"
                                    ? "CI 制品 → SSH"
                                    : "本机构建 → 本机"
                            }}
                        </span>
                    </div>

                    <dl class="fields">
                        <div>
                            <dt>制品来源</dt>
                            <dd>{{ project.repo_path }}</dd>
                        </div>
                        <div>
                            <dt>分支</dt>
                            <dd>{{ project.branch }}</dd>
                        </div>
                        <div>
                            <dt>目标服务器</dt>
                            <dd v-if="project.deploy_mode === 'local'" class="is-nil">
                                本机部署
                            </dd>
                            <dd v-else-if="serverName(project)" class="is-link">
                                {{ serverName(project) }}
                            </dd>
                            <dd v-else class="is-nil">未选择服务器</dd>
                        </div>
                        <div>
                            <dt>部署目录</dt>
                            <dd>{{ project.deploy_path }}</dd>
                        </div>
                    </dl>

                    <div class="card-f">
                        <span class="card-last mono">{{ project.branch }}</span>
                        <el-button size="small" @click="runDeploy(project)">部署</el-button>
                        <el-button size="small" @click="openEdit(project)">编辑</el-button>
                    </div>
                </article>
            </div>
        </section>

        <el-dialog
            v-model="dialogVisible"
            :title="dialogMode === 'create' ? '新建项目' : '编辑项目'"
            width="640px"
            destroy-on-close
        >
            <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
                <div class="group">
                    <div class="group-t"><span class="cap">01 · 基本信息</span></div>
                    <div class="two">
                        <el-form-item label="项目名称" prop="name">
                            <el-input v-model="form.name" placeholder="myapp" />
                        </el-form-item>
                        <el-form-item label="部署方式">
                            <el-select v-model="form.deploy_mode" style="width: 100%">
                                <el-option label="CI 制品 + SSH" value="ci_ssh" />
                                <el-option label="本机构建" value="local" />
                            </el-select>
                        </el-form-item>
                    </div>
                    <div class="two">
                        <el-form-item label="仓库标识或路径" prop="repo_path">
                            <el-input
                                v-model="form.repo_path"
                                :placeholder="
                                    isRemoteMode
                                        ? 'owner/repository'
                                        : 'D:\\\\repos\\\\myapp 或 https://...'
                                "
                            />
                        </el-form-item>
                        <el-form-item label="部署分支" prop="branch">
                            <el-input v-model="form.branch" placeholder="main" />
                        </el-form-item>
                    </div>
                </div>

                <div v-if="isRemoteMode" class="group">
                    <div class="group-t"><span class="cap">02 · 目标服务器</span></div>
                    <el-form-item label="服务器" prop="server_id">
                        <el-select
                            v-model="form.server_id"
                            clearable
                            placeholder="选择一台服务器"
                            style="width: 100%"
                        >
                            <el-option
                                v-for="server in selectableServers"
                                :key="server.id"
                                :label="`${server.name} · ${server.username}@${server.host}:${server.port}`"
                                :value="server.id"
                            />
                        </el-select>
                        <div class="hint">
                            连接信息（主机、用户、密钥或密码）在服务器页维护，这里只做引用。
                            <RouterLink to="/servers">去管理服务器 →</RouterLink>
                        </div>
                    </el-form-item>
                    <el-form-item label="远程部署目录" prop="deploy_path">
                        <el-input v-model="form.deploy_path" placeholder="/srv/apps/myapp" />
                    </el-form-item>
                </div>

                <div v-else class="group">
                    <div class="group-t"><span class="cap">02 · 本机构建</span></div>
                    <el-form-item label="构建工作目录（可选，相对仓库根目录）">
                        <el-input v-model="form.workdir" placeholder="packages/web" />
                    </el-form-item>
                    <el-form-item label="构建命令" prop="build_command">
                        <el-input v-model="form.build_command" placeholder="pnpm build" />
                    </el-form-item>
                    <div class="two">
                        <el-form-item label="产物路径" prop="built_path">
                            <el-input v-model="form.built_path" placeholder="dist" />
                        </el-form-item>
                        <el-form-item label="本机部署目录" prop="deploy_path">
                            <el-input v-model="form.deploy_path" placeholder="D:\\\\apps\\\\myapp" />
                        </el-form-item>
                    </div>
                </div>

                <div class="group">
                    <div class="group-t"><span class="cap">03 · 服务生命周期（可选）</span></div>
                    <div class="two">
                        <el-form-item label="停止命令">
                            <el-input v-model="form.stop_command" placeholder="pm2 stop myapp" />
                        </el-form-item>
                        <el-form-item label="启动命令">
                            <el-input v-model="form.start_command" placeholder="pm2 start myapp" />
                        </el-form-item>
                    </div>
                </div>

                <el-form-item>
                    <el-checkbox v-model="form.enabled">
                        启用（webhook 与手动部署都会生效）
                    </el-checkbox>
                </el-form-item>
            </el-form>

            <template #footer>
                <el-button @click="dialogVisible = false">取消</el-button>
                <el-button type="primary" :loading="saving" @click="saveProject">
                    保存项目
                </el-button>
            </template>
        </el-dialog>
    </div>
</template>

<style scoped>
.content { padding: 20px 24px 44px; }
.search { width: 220px; margin-right: 8px; }

.state,
.empty {
    display: flex;
    min-height: 150px;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: 10px;
    color: var(--tx3);
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
    font-size: 13px;
}

.empty strong { color: var(--tx); font-size: 14px; }
.empty p { margin: 0 0 4px; font-size: 12px; }

.grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(322px, 1fr));
    gap: 11px;
}

.card {
    display: flex;
    flex-direction: column;
    background: var(--sf);
    border: 1px solid var(--bd);
    border-radius: var(--r-lg);
    transition: border-color 120ms;
}

.card:hover { border-color: var(--bd2); }
.card-h { display: flex; align-items: center; gap: 9px; padding: 12px 13px 0; }
.card-n {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    font-size: 13.5px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.card-kind { padding: 7px 13px 0; }

.fields {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 11px 13px;
    border-bottom: 1px solid var(--bd);
}

.fields dt {
    color: var(--tx4);
    font-family: var(--font-mono);
    font-size: 9.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
}

.fields dd {
    margin: 2px 0 0;
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.fields dd.is-nil { color: var(--tx4); }
.fields dd.is-link { color: var(--tag); }

.card-f { display: flex; align-items: center; gap: 7px; padding: 9px 13px; }
.card-last { margin-right: auto; color: var(--tx3); font-size: 10.5px; }

.group { margin-bottom: 16px; }
.group-t {
    margin-bottom: 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--bd);
}
.two { display: grid; grid-template-columns: 1fr 1fr; gap: 0 12px; }

.hint {
    margin-top: 4px;
    color: var(--tx3);
    font-size: 11.5px;
    line-height: 1.5;
}

.hint a { color: var(--tag); }

@media (max-width: 760px) {
    .content { padding: 14px 12px 34px; }
    .search { width: 100%; margin: 0 0 8px; }
    .two { grid-template-columns: 1fr; }
}
</style>
