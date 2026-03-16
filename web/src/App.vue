<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import {
    changePassword,
    clearToken,
    createProject,
    deleteProject,
    deployProject,
    getDeploymentDetail,
    getToken,
    getWebhookSecret,
    listDeployments,
    listProjects,
    login,
    me,
    setToken,
    updateWebhookSecret,
    updateProject,
    type Deployment,
    type DeploymentDetail,
    type Project,
} from "./api";

const loading = ref(false);
const error = ref("");
const userName = ref("");
const loggedIn = ref(false);
const projects = ref<Project[]>([]);
const deployments = ref<Deployment[]>([]);
const selectedProjectId = ref<number | null>(null);
const selectedDeployment = ref<DeploymentDetail | null>(null);
const webhookSecret = ref("");

const loginForm = reactive({
    username: "admin",
    password: "admin",
});

const pwdForm = reactive({
    old_password: "",
    new_password: "",
});

const projectForm = reactive({
    id: 0,
    name: "",
    repo_path: "",
    branch: "main",
    workdir: "",
    build_command: "",
    built_path: "",
    deploy_path: "",
    stop_command: "",
    start_command: "",
    enabled: true,
});

const editing = computed(() => projectForm.id > 0);

function resetProjectForm() {
    projectForm.id = 0;
    projectForm.name = "";
    projectForm.repo_path = "";
    projectForm.branch = "main";
    projectForm.workdir = "";
    projectForm.build_command = "";
    projectForm.built_path = "";
    projectForm.deploy_path = "";
    projectForm.stop_command = "";
    projectForm.start_command = "";
    projectForm.enabled = true;
}

function fillForm(project: Project) {
    projectForm.id = project.id;
    projectForm.name = project.name;
    projectForm.repo_path = project.repo_path;
    projectForm.branch = project.branch;
    projectForm.workdir = project.workdir || "";
    projectForm.build_command = project.build_command;
    projectForm.built_path = project.built_path;
    projectForm.deploy_path = project.deploy_path;
    projectForm.stop_command = project.stop_command || "";
    projectForm.start_command = project.start_command || "";
    projectForm.enabled = project.enabled;
}

function toPayload() {
    return {
        name: projectForm.name,
        repo_path: projectForm.repo_path,
        branch: projectForm.branch,
        workdir: projectForm.workdir || null,
        build_command: projectForm.build_command,
        built_path: projectForm.built_path,
        deploy_path: projectForm.deploy_path,
        stop_command: projectForm.stop_command || null,
        start_command: projectForm.start_command || null,
        enabled: projectForm.enabled,
    };
}

async function doLogin() {
    loading.value = true;
    error.value = "";
    try {
        const result = await login(loginForm.username, loginForm.password);
        setToken(result.token);
        userName.value = result.user.username;
        loggedIn.value = true;
        await refreshData();
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function refreshData() {
    const [p, d] = await Promise.all([listProjects(), listDeployments(30)]);
    projects.value = p;
    deployments.value = d;
    if (p.length > 0 && selectedProjectId.value == null) {
        selectedProjectId.value = p[0].id;
    }
    await loadWebhookSecret();
}

async function doLogout() {
    clearToken();
    loggedIn.value = false;
    userName.value = "";
    projects.value = [];
    deployments.value = [];
}

async function submitProject() {
    loading.value = true;
    error.value = "";
    try {
        if (editing.value) {
            await updateProject(projectForm.id, toPayload());
        } else {
            await createProject(toPayload());
        }
        await refreshData();
        resetProjectForm();
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function doDeploy(id: number) {
    loading.value = true;
    error.value = "";
    try {
        await deployProject(id);
        await refreshData();
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function doDeleteProject(id: number) {
    loading.value = true;
    error.value = "";
    try {
        await deleteProject(id);
        await refreshData();
        if (projectForm.id === id) {
            resetProjectForm();
        }
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function openDeploymentDetail(id: number) {
    loading.value = true;
    error.value = "";
    try {
        selectedDeployment.value = await getDeploymentDetail(id);
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function submitPassword() {
    loading.value = true;
    error.value = "";
    try {
        await changePassword(pwdForm.old_password, pwdForm.new_password);
        pwdForm.old_password = "";
        pwdForm.new_password = "";
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function loadWebhookSecret() {
    try {
        const data = await getWebhookSecret();
        webhookSecret.value = data.webhook_secret;
    } catch {
        // ignore here to avoid blocking other data
    }
}

async function submitWebhookSecret() {
    loading.value = true;
    error.value = "";
    try {
        await updateWebhookSecret(webhookSecret.value);
        await loadWebhookSecret();
    } catch (e) {
        error.value = (e as Error).message;
    } finally {
        loading.value = false;
    }
}

async function bootstrap() {
    const token = getToken();
    if (!token) {
        return;
    }
    try {
        const user = await me();
        userName.value = user.username;
        loggedIn.value = true;
        await refreshData();
    } catch {
        clearToken();
    }
}

onMounted(bootstrap);
</script>

<template>
    <div class="page">
        <h1>Auto Deploy 管理台</h1>
        <p class="subtitle">server: Rust + SQLite | web: Vue3 + TypeScript</p>

        <div v-if="!loggedIn" class="card">
            <h2>登录</h2>
            <div class="grid">
                <label>用户名</label>
                <input v-model="loginForm.username" autocomplete="username" />
                <label>密码</label>
                <input
                    v-model="loginForm.password"
                    type="password"
                    autocomplete="current-password"
                />
            </div>
            <button :disabled="loading" @click="doLogin">登录</button>
            <p class="hint">默认账号密码：admin / admin</p>
        </div>

        <template v-else>
            <div class="toolbar">
                <span>当前用户：{{ userName }}</span>
                <button class="ghost" :disabled="loading" @click="doLogout">
                    退出登录
                </button>
            </div>

            <div class="layout">
                <section class="card">
                    <h2>{{ editing ? "编辑项目" : "新增项目" }}</h2>
                    <div class="grid">
                        <label>名称</label>
                        <input v-model="projectForm.name" />
                        <label>仓库路径</label>
                        <input v-model="projectForm.repo_path" />
                        <label>分支</label>
                        <input v-model="projectForm.branch" />
                        <label>工作目录</label>
                        <input
                            v-model="projectForm.workdir"
                            placeholder="留空=repo_path"
                        />
                        <label>构建命令</label>
                        <input v-model="projectForm.build_command" />
                        <label>构建产物路径</label>
                        <input v-model="projectForm.built_path" />
                        <label>部署目标路径</label>
                        <input v-model="projectForm.deploy_path" />
                        <label>停止命令</label>
                        <input v-model="projectForm.stop_command" />
                        <label>启动命令</label>
                        <input v-model="projectForm.start_command" />
                    </div>
                    <label class="inline">
                        <input v-model="projectForm.enabled" type="checkbox" />
                        启用
                    </label>
                    <div class="actions">
                        <button :disabled="loading" @click="submitProject">
                            {{ editing ? "保存更新" : "创建项目" }}
                        </button>
                        <button
                            v-if="editing"
                            class="ghost"
                            :disabled="loading"
                            @click="resetProjectForm"
                        >
                            取消编辑
                        </button>
                    </div>
                </section>

                <section class="card">
                    <h2>项目列表</h2>
                    <table>
                        <thead>
                            <tr>
                                <th>ID</th>
                                <th>名称</th>
                                <th>分支</th>
                                <th>状态</th>
                                <th>操作</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="item in projects" :key="item.id">
                                <td>{{ item.id }}</td>
                                <td>{{ item.name }}</td>
                                <td>{{ item.branch }}</td>
                                <td>{{ item.enabled ? "启用" : "停用" }}</td>
                                <td>
                                    <button
                                        class="small"
                                        :disabled="loading"
                                        @click="fillForm(item)"
                                    >
                                        编辑
                                    </button>
                                    <button
                                        class="small"
                                        :disabled="loading"
                                        @click="doDeploy(item.id)"
                                    >
                                        手动部署
                                    </button>
                                    <button
                                        class="small ghost"
                                        :disabled="loading"
                                        @click="doDeleteProject(item.id)"
                                    >
                                        删除
                                    </button>
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </section>
            </div>

            <div class="layout">
                <section class="card">
                    <h2>部署记录</h2>
                    <button
                        class="ghost"
                        :disabled="loading"
                        @click="refreshData"
                    >
                        刷新
                    </button>
                    <table>
                        <thead>
                            <tr>
                                <th>ID</th>
                                <th>项目</th>
                                <th>分支</th>
                                <th>状态</th>
                                <th>开始时间</th>
                                <th>结果</th>
                                <th>日志</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="item in deployments" :key="item.id">
                                <td>{{ item.id }}</td>
                                <td>{{ item.project_name }}</td>
                                <td>{{ item.branch }}</td>
                                <td>{{ item.status }}</td>
                                <td>{{ item.started_at }}</td>
                                <td>{{ item.message || "-" }}</td>
                                <td>
                                    <button
                                        class="small ghost"
                                        :disabled="loading"
                                        @click="openDeploymentDetail(item.id)"
                                    >
                                        查看
                                    </button>
                                </td>
                            </tr>
                        </tbody>
                    </table>
                    <div v-if="selectedDeployment" class="log-box">
                        <h3>部署日志 #{{ selectedDeployment.id }}</h3>
                        <pre>{{
                            selectedDeployment.log_text || "暂无日志"
                        }}</pre>
                    </div>
                </section>

                <section class="card">
                    <h2>修改密码</h2>
                    <div class="grid">
                        <label>旧密码</label>
                        <input v-model="pwdForm.old_password" type="password" />
                        <label>新密码</label>
                        <input v-model="pwdForm.new_password" type="password" />
                    </div>
                    <button :disabled="loading" @click="submitPassword">
                        更新密码
                    </button>

                    <h2 style="margin-top: 16px">Webhook Secret</h2>
                    <div class="grid">
                        <label>Secret</label>
                        <input
                            v-model="webhookSecret"
                            type="text"
                            placeholder="留空表示不校验签名"
                        />
                    </div>
                    <button :disabled="loading" @click="submitWebhookSecret">
                        保存 Secret
                    </button>
                </section>
            </div>
        </template>

        <p v-if="error" class="error">{{ error }}</p>
    </div>
</template>
