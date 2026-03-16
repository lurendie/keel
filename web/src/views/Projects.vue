<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import {
  Box,
  Delete,
  EditPen,
  Key,
  Link,
  Plus,
  Promotion,
  Search,
  VideoPlay,
} from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox, type FormInstance, type FormRules } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import StatusPill from "../components/StatusPill.vue";
import {
  createProject,
  deleteProject,
  deployProject,
  listProjects,
  updateProject,
  type Project,
  type ProjectPayload,
} from "../api/api";

const projects = ref<Project[]>([]);
const loading = ref(true);
const saving = ref(false);
const dialogVisible = ref(false);
const dialogMode = ref<"create" | "edit">("create");
const searchText = ref("");
const editingId = ref<number | null>(null);
const projectFormRef = ref<FormInstance>();
const passphraseConfigured = ref(false);
const clearPassphrase = ref(false);

const emptyProject = (): ProjectPayload => ({
  name: "",
  deploy_mode: "local",
  repo_path: "",
  branch: "main",
  workdir: null,
  build_command: "",
  built_path: "",
  artifact_url: null,
  artifact_sha256: null,
  ssh_host: null,
  ssh_port: 22,
  ssh_user: null,
  ssh_private_key_path: null,
  ssh_private_key_passphrase: null,
  deploy_path: "",
  stop_command: null,
  start_command: null,
  enabled: true,
});

const form = reactive<ProjectPayload>(emptyProject());

const rules: FormRules<ProjectPayload> = {
  name: [{ required: true, message: "请输入项目名称", trigger: "blur" }],
  repo_path: [{ required: true, message: "请输入仓库标识或本地仓库路径", trigger: "blur" }],
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
  ssh_host: [
    {
      validator: (_rule, value, callback) => {
        if (form.deploy_mode === "ci_ssh" && !String(value || "").trim()) {
          callback(new Error("请输入 SSH 主机"));
          return;
        }
        callback();
      },
      trigger: "blur",
    },
  ],
  ssh_user: [
    {
      validator: (_rule, value, callback) => {
        if (form.deploy_mode === "ci_ssh" && !String(value || "").trim()) {
          callback(new Error("请输入 SSH 用户"));
          return;
        }
        callback();
      },
      trigger: "blur",
    },
  ],
  ssh_private_key_path: [
    {
      validator: (_rule, value, callback) => {
        if (form.deploy_mode === "ci_ssh" && !String(value || "").trim()) {
          callback(new Error("请输入私钥路径"));
          return;
        }
        callback();
      },
      trigger: "blur",
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
    [project.name, project.repo_path, project.branch, project.ssh_host || ""]
      .join(" ")
      .toLowerCase()
      .includes(keyword),
  );
});

const loadProjects = async () => {
  loading.value = true;
  try {
    projects.value = await listProjects();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "项目加载失败");
  } finally {
    loading.value = false;
  }
};

const resetForm = () => {
  Object.assign(form, emptyProject());
  editingId.value = null;
  passphraseConfigured.value = false;
  clearPassphrase.value = false;
};

const openCreate = () => {
  dialogMode.value = "create";
  resetForm();
  dialogVisible.value = true;
};

const openEdit = (project: Project) => {
  dialogMode.value = "edit";
  editingId.value = project.id;
  Object.assign(form, {
    name: project.name,
    deploy_mode: project.deploy_mode,
    repo_path: project.repo_path,
    branch: project.branch,
    workdir: null,
    build_command: project.build_command,
    built_path: project.built_path,
    artifact_url: project.artifact_url,
    artifact_sha256: project.artifact_sha256,
    ssh_host: project.ssh_host,
    ssh_port: project.ssh_port || 22,
    ssh_user: project.ssh_user,
    ssh_private_key_path: project.ssh_private_key_path,
    ssh_private_key_passphrase: "",
    deploy_path: project.deploy_path,
    stop_command: project.stop_command,
    start_command: project.start_command,
    enabled: project.enabled,
  });
  passphraseConfigured.value = project.ssh_private_key_passphrase_set;
  clearPassphrase.value = false;
  dialogVisible.value = true;
};

const saveProject = async () => {
  if (!projectFormRef.value) {
    return;
  }

  try {
    await projectFormRef.value.validate();
    saving.value = true;

    const remoteMode = form.deploy_mode === "ci_ssh";
    const payload: ProjectPayload = {
      ...form,
      repo_path: form.repo_path.trim(),
      build_command: remoteMode ? "" : form.build_command.trim(),
      built_path: remoteMode ? "" : form.built_path.trim(),
      artifact_url: remoteMode ? form.artifact_url?.trim() || null : null,
      artifact_sha256: remoteMode ? form.artifact_sha256?.trim() || null : null,
      ssh_host: remoteMode ? form.ssh_host?.trim() || null : null,
      ssh_user: remoteMode ? form.ssh_user?.trim() || null : null,
      ssh_private_key_path: remoteMode ? form.ssh_private_key_path?.trim() || null : null,
      ssh_private_key_passphrase: remoteMode
        ? clearPassphrase.value
          ? ""
          : form.ssh_private_key_passphrase || null
        : null,
      workdir: null,
      stop_command: form.stop_command?.trim() || null,
      start_command: form.start_command?.trim() || null,
    };

    if (dialogMode.value === "create") {
      await createProject(payload);
      ElMessage.success("项目已创建");
    } else if (editingId.value) {
      await updateProject(editingId.value, payload);
      ElMessage.success("项目已更新");
    }

    dialogVisible.value = false;
    await loadProjects();
  } catch (error) {
    if (error instanceof Error) {
      ElMessage.error(error.message);
    }
  } finally {
    saving.value = false;
  }
};

const removeProject = async (project: Project) => {
  try {
    await ElMessageBox.confirm(
      `删除「${project.name}」会同时移除它的部署记录，是否继续？`,
      "删除项目",
      {
        type: "warning",
        confirmButtonText: "删除",
        cancelButtonText: "取消",
      },
    );
    await deleteProject(project.id);
    ElMessage.success("项目已删除");
    await loadProjects();
  } catch (error) {
    if (error !== "cancel" && error !== "close") {
      ElMessage.error(error instanceof Error ? error.message : "删除项目失败");
    }
  }
};

const runDeploy = async (project: Project) => {
  try {
    await ElMessageBox.confirm(
      `立即为「${project.name}」创建一次部署任务？`,
      "手动部署",
      {
        confirmButtonText: "开始部署",
        cancelButtonText: "取消",
      },
    );
    await deployProject(project.id);
    ElMessage.success("部署任务已创建");
  } catch (error) {
    if (error !== "cancel" && error !== "close") {
      ElMessage.error(error instanceof Error ? error.message : "创建部署任务失败");
    }
  }
};

onMounted(loadProjects);
</script>

<template>
    <div class="projects-page">
        <PageIntro
            eyebrow="Workspace / Projects"
            title="项目管理"
            description="定义制品从哪里来、送往哪台服务器，以及一次发布需要执行的停机、启动命令。"
        >
            <template #actions>
                <el-button type="primary" @click="openCreate">
                    <el-icon><Plus /></el-icon>
                    新建项目
                </el-button>
            </template>
        </PageIntro>

        <section class="projects-content">
            <div class="projects-toolbar">
                <div class="toolbar-count">
                    <strong>{{ filteredProjects.length }}</strong>
                    <span>个项目</span>
                </div>
                <el-input
                    v-model="searchText"
                    class="search-input"
                    clearable
                    placeholder="搜索项目、分支或服务器"
                >
                    <template #prefix><el-icon><Search /></el-icon></template>
                </el-input>
            </div>

            <div v-if="loading" class="state-panel">正在读取项目配置...</div>
            <div v-else-if="filteredProjects.length === 0" class="empty-panel">
                <div class="empty-symbol"><Box /></div>
                <strong>{{ searchText ? "没有匹配的项目" : "还没有项目" }}</strong>
                <p>{{ searchText ? "换一个关键词试试。" : "创建项目后，就可以接入 CI 制品和 SSH 服务器。" }}</p>
                <el-button v-if="!searchText" type="primary" @click="openCreate">
                    <el-icon><Plus /></el-icon>
                    创建第一个项目
                </el-button>
            </div>
            <div v-else class="project-table-wrap">
                <el-table :data="filteredProjects" class="project-table" table-layout="fixed">
                    <el-table-column min-width="220" label="项目">
                        <template #default="{ row }">
                            <div class="project-name-cell">
                                <div class="project-avatar">{{ row.name.slice(0, 1).toUpperCase() }}</div>
                                <div>
                                    <strong>{{ row.name }}</strong>
                                    <span>{{ row.repo_path }}</span>
                                </div>
                            </div>
                        </template>
                    </el-table-column>
                    <el-table-column min-width="145" label="发布源">
                        <template #default="{ row }">
                            <div class="source-cell">
                                <StatusPill
                                    :status="row.deploy_mode === 'ci_ssh' ? 'healthy' : 'neutral'"
                                    :label="row.deploy_mode === 'ci_ssh' ? 'CI artifact' : 'Local build'"
                                />
                                <span>{{ row.branch }}</span>
                            </div>
                        </template>
                    </el-table-column>
                    <el-table-column min-width="180" label="SSH 目标">
                        <template #default="{ row }">
                            <div v-if="row.ssh_host" class="target-cell">
                                <span>{{ row.ssh_user }}@{{ row.ssh_host }}</span>
                                <small>{{ row.deploy_path }}</small>
                            </div>
                            <span v-else class="muted-text">本机部署</span>
                        </template>
                    </el-table-column>
                    <el-table-column width="100" label="状态">
                        <template #default="{ row }">
                            <StatusPill :status="row.enabled ? 'enabled' : 'disabled'" :label="row.enabled ? '已启用' : '已停用'" />
                        </template>
                    </el-table-column>
                    <el-table-column width="205" label="操作" align="right">
                        <template #default="{ row }">
                            <div class="row-actions">
                                <el-button text title="立即部署" @click="runDeploy(row)">
                                    <el-icon><VideoPlay /></el-icon>
                                </el-button>
                                <el-button text title="编辑项目" @click="openEdit(row)">
                                    <el-icon><EditPen /></el-icon>
                                </el-button>
                                <el-button text class="danger-action" title="删除项目" @click="removeProject(row)">
                                    <el-icon><Delete /></el-icon>
                                </el-button>
                            </div>
                        </template>
                    </el-table-column>
                </el-table>
            </div>
        </section>

        <el-dialog
            v-model="dialogVisible"
            :title="dialogMode === 'create' ? '新建项目' : '编辑项目'"
            width="720px"
            class="project-dialog"
            destroy-on-close
        >
            <el-form
                ref="projectFormRef"
                :model="form"
                :rules="rules"
                label-position="top"
                class="project-form"
            >
                <div class="form-section">
                    <div class="form-section-heading">
                        <span class="form-section-index">01</span>
                        <div>
                            <strong>项目身份</strong>
                            <p>用于匹配 webhook 事件和区分部署任务。</p>
                        </div>
                    </div>
                    <div class="form-grid two-columns">
                        <el-form-item label="项目名称" prop="name">
                            <el-input v-model="form.name" placeholder="例如：商城前端" />
                        </el-form-item>
                        <el-form-item
                            :label="isRemoteMode ? '仓库标识' : '本地路径 / 远程 Git 地址'"
                            prop="repo_path"
                        >
                            <el-input
                                v-model="form.repo_path"
                                :placeholder="
                                    isRemoteMode
                                        ? 'org/repository'
                                        : 'D:\apps\repo 或 https://github.com/org/repo.git'
                                "
                            />
                        </el-form-item>
                        <el-form-item label="部署分支" prop="branch">
                            <el-input v-model="form.branch" placeholder="main" />
                        </el-form-item>
                    </div>
                </div>

                <div class="form-section">
                    <div class="form-section-heading">
                        <span class="form-section-index">02</span>
                        <div>
                            <strong>发布源</strong>
                            <p>选择由部署服务本机构建，还是下载 CI 已构建的制品。</p>
                        </div>
                    </div>
                    <el-form-item label="部署方式">
                        <el-radio-group v-model="form.deploy_mode" class="mode-switch">
                            <el-radio-button value="local">本机构建部署</el-radio-button>
                            <el-radio-button value="ci_ssh">CI 制品 + SSH</el-radio-button>
                        </el-radio-group>
                    </el-form-item>
                    <div class="form-grid two-columns">
                        <el-form-item v-if="!isRemoteMode" label="本机构建命令">
                            <el-input v-model="form.build_command" placeholder="npm ci && npm run build" />
                        </el-form-item>
                        <el-form-item v-if="!isRemoteMode" label="本地产物路径">
                            <el-input v-model="form.built_path" placeholder="dist" />
                        </el-form-item>
                        <el-form-item v-if="!isRemoteMode" label="本地部署目录" prop="deploy_path">
                            <el-input v-model="form.deploy_path" placeholder="D:\apps\shop" />
                        </el-form-item>
                        <el-form-item v-if="isRemoteMode" label="默认 CI 制品下载地址">
                            <el-input
                                v-model="form.artifact_url"
                                placeholder="可留空，由 Release 或 CI 回调动态传入"
                            >
                                <template #prefix><el-icon><Link /></el-icon></template>
                            </el-input>
                        </el-form-item>
                        <el-form-item v-if="isRemoteMode" label="SHA-256 校验值">
                            <el-input v-model="form.artifact_sha256" placeholder="可选，建议填写" />
                        </el-form-item>
                    </div>
                    <div class="form-note">
                        <el-icon><Promotion /></el-icon>
                        {{
                            isRemoteMode
                                ? form.artifact_url?.trim()
                                    ? "本机不会执行构建；Release 和 CI 回调可以覆盖默认制品地址。"
                                    : "制品地址为空，只能通过 Release 事件或 CI 回调发起部署。"
                                : "本地路径直接拉取更新；远程 Git 地址会先克隆到受管工作区，再执行构建。"
                        }}
                    </div>
                </div>

                <div v-if="isRemoteMode" class="form-section">
                    <div class="form-section-heading">
                        <span class="form-section-index">03</span>
                        <div>
                            <strong>SSH 服务器</strong>
                            <p>部署服务使用密钥认证，私钥只保存路径，不上传到服务器。</p>
                        </div>
                    </div>
                    <div class="form-grid four-columns">
                        <el-form-item label="主机">
                            <el-input v-model="form.ssh_host" placeholder="192.168.1.10" />
                        </el-form-item>
                        <el-form-item label="端口">
                            <el-input-number v-model="form.ssh_port" :min="1" :max="65535" controls-position="right" />
                        </el-form-item>
                        <el-form-item label="用户">
                            <el-input v-model="form.ssh_user" placeholder="deploy" />
                        </el-form-item>
                        <el-form-item label="私钥路径">
                            <el-input v-model="form.ssh_private_key_path" placeholder="~/.ssh/id_ed25519">
                                <template #prefix><el-icon><Key /></el-icon></template>
                            </el-input>
                        </el-form-item>
                    </div>
                    <div class="form-grid two-columns">
                        <el-form-item label="私钥密码">
                            <div class="passphrase-field">
                                <el-input
                                    v-model="form.ssh_private_key_passphrase"
                                    type="password"
                                    show-password
                                    :disabled="clearPassphrase"
                                    :placeholder="passphraseConfigured ? '留空则保持现有密码' : '可选'"
                                />
                                <el-checkbox
                                    v-if="dialogMode === 'edit' && passphraseConfigured"
                                    v-model="clearPassphrase"
                                >
                                    清除已保存的私钥密码
                                </el-checkbox>
                            </div>
                        </el-form-item>
                        <el-form-item label="服务器部署目录" prop="deploy_path">
                            <el-input v-model="form.deploy_path" placeholder="/srv/apps/shop" />
                        </el-form-item>
                    </div>
                </div>

                <div class="form-section form-section-last">
                    <div class="form-section-heading">
                        <span class="form-section-index">{{ isRemoteMode ? "04" : "03" }}</span>
                        <div>
                            <strong>发布动作</strong>
                            <p>
                                {{
                                    isRemoteMode
                                        ? "命令会在远程服务器执行，请使用可重复运行的脚本。"
                                        : "命令会在部署服务所在机器执行。"
                                }}
                            </p>
                        </div>
                    </div>
                    <div class="form-grid two-columns">
                        <el-form-item label="停止命令">
                            <el-input v-model="form.stop_command" placeholder="sudo systemctl stop shop" />
                        </el-form-item>
                        <el-form-item label="启动命令">
                            <el-input v-model="form.start_command" placeholder="sudo systemctl start shop" />
                        </el-form-item>
                    </div>
                    <div class="enabled-line">
                        <div>
                            <strong>启用自动部署</strong>
                            <span>关闭后仍保留配置，但不会响应 webhook。</span>
                        </div>
                        <el-switch v-model="form.enabled" />
                    </div>
                </div>
            </el-form>
            <template #footer>
                <el-button @click="dialogVisible = false">取消</el-button>
                <el-button type="primary" :loading="saving" @click="saveProject">保存项目</el-button>
            </template>
        </el-dialog>
    </div>
</template>

<style scoped>
.projects-page {
    min-height: 100%;
}

.projects-content {
    padding: 28px 36px 44px;
}

.projects-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
}

.toolbar-count {
    display: flex;
    align-items: baseline;
    gap: 7px;
    color: #88928a;
    font-size: 12px;
}

.toolbar-count strong {
    color: #354139;
    font-family: Georgia, serif;
    font-size: 24px;
    font-weight: 500;
}

.search-input {
    width: min(300px, 48%);
}

.project-table-wrap,
.empty-panel,
.state-panel {
    background: #ffffff;
    border: 1px solid #d9ded9;
}

.project-table {
    --el-table-border-color: #e1e5e1;
    --el-table-header-bg-color: #eef1ed;
    --el-table-row-hover-bg-color: #f5f5f5;
    --el-table-bg-color: #ffffff;
    --el-fill-color-blank: #ffffff;
    color: #354139;
}

.project-table :deep(.el-table__header-wrapper th) {
    height: 42px;
    color: #7b877e;
    font-size: 11px;
    font-weight: 750;
    letter-spacing: 0.06em;
}

.project-table :deep(.el-table__row td) {
    height: 72px;
}

.project-name-cell,
.source-cell,
.target-cell {
    display: flex;
    align-items: center;
}

.project-name-cell {
    gap: 11px;
}

.project-avatar {
    display: grid;
    flex-shrink: 0;
    width: 34px;
    height: 34px;
    place-items: center;
    color: #404040;
    background: #f0f0f0;
    font-size: 12px;
    font-weight: 800;
}

.project-name-cell > div:last-child,
.target-cell {
    display: flex;
    min-width: 0;
    flex-direction: column;
}

.project-name-cell strong,
.target-cell span {
    overflow: hidden;
    color: #354139;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.project-name-cell span,
.target-cell small {
    overflow: hidden;
    margin-top: 4px;
    color: #929b94;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.source-cell {
    gap: 8px;
    flex-wrap: wrap;
}

.source-cell > span:last-child {
    color: #68746b;
    font-size: 11px;
}

.muted-text {
    color: #a0a8a1;
    font-size: 12px;
}

.row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
}

.row-actions .el-button {
    width: 30px;
    height: 30px;
    margin: 0;
    color: #657169;
}

.row-actions .el-button:hover {
    color: #171717;
    background: #f0f0f0;
}

.row-actions .danger-action:hover {
    color: #a94d42;
    background: #f8e6e1;
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
    margin: 0 0 6px;
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

.form-section {
    padding: 2px 0 22px;
    border-bottom: 1px solid #e2e6e2;
}

.form-section + .form-section {
    margin-top: 22px;
}

.form-section-last {
    padding-bottom: 0;
    border-bottom: 0;
}

.form-section-heading {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 17px;
}

.form-section-index {
    color: #737373;
    font-family: "Arial Narrow", "Segoe UI", sans-serif;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.12em;
}

.form-section-heading strong {
    color: #354139;
    font-size: 13px;
}

.form-section-heading p {
    margin: 4px 0 0;
    color: #929b94;
    font-size: 11px;
}

.form-grid {
    display: grid;
    gap: 0 14px;
}

.mode-switch {
    display: flex;
    width: 100%;
}

.mode-switch :deep(.el-radio-button) {
    flex: 1;
}

.mode-switch :deep(.el-radio-button__inner) {
    width: 100%;
}

.two-columns {
    grid-template-columns: repeat(2, minmax(0, 1fr));
}

.four-columns {
    grid-template-columns: 1.4fr 0.65fr 1fr 1.6fr;
}

.project-form :deep(.el-form-item) {
    margin-bottom: 14px;
}

.project-form :deep(.el-form-item__label) {
    height: auto;
    margin-bottom: 6px;
    padding: 0;
    color: #66736a;
    font-size: 11px;
    line-height: 1.2;
}

.project-form :deep(.el-input-number) {
    width: 100%;
}

.passphrase-field {
    width: 100%;
}

.passphrase-field :deep(.el-checkbox) {
    margin-top: 7px;
    color: #7b877e;
    font-size: 11px;
}

.form-note {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 3px;
    padding: 9px 11px;
    color: #525252;
    background: #f5f5f5;
    font-size: 11px;
}

.enabled-line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: 4px;
}

.enabled-line > div {
    display: flex;
    flex-direction: column;
}

.enabled-line strong {
    color: #354139;
    font-size: 12px;
}

.enabled-line span {
    margin-top: 4px;
    color: #929b94;
    font-size: 11px;
}

@media (max-width: 920px) {
    .projects-content {
        padding: 24px 24px 36px;
    }

    .four-columns {
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }
}

@media (max-width: 720px) {
    .projects-content {
        padding: 20px 18px 32px;
    }

    .projects-toolbar {
        align-items: stretch;
        gap: 14px;
        flex-direction: column;
    }

    .search-input {
        width: 100%;
    }

    .project-table-wrap {
        overflow-x: auto;
    }

    .project-table {
        min-width: 780px;
    }

    .two-columns,
    .four-columns {
        grid-template-columns: 1fr;
    }
}
</style>
