<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ElMessage, ElMessageBox, type FormRules } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import ServerForm from "../components/ServerForm.vue";
import {
  createServer,
  deleteServer,
  listServers,
  listProjects,
  testServer,
  updateServer,
  type Project,
  type Server,
  type ServerPayload,
} from "../api/api";

const servers = ref<Server[]>([]);
const projects = ref<Project[]>([]);
const loading = ref(true);
const saving = ref(false);
const error = ref("");
const dialogVisible = ref(false);
const editingId = ref<number | null>(null);
const formRef = ref<InstanceType<typeof ServerForm>>();
const testing = ref<number | null>(null);
const testResults = reactive<Record<number, string>>({});

const emptyServer = (): ServerPayload => ({
  name: "",
  group_name: null,
  host: "",
  port: 22,
  username: "deploy",
  auth_method: "key",
  private_key_path: null,
  private_key_passphrase: null,
  password: null,
  deploy_path: null,
  enabled: true,
});

const form = reactive<ServerPayload>(emptyServer());

const rules: FormRules<ServerPayload> = {
  name: [{ required: true, message: "请输入服务器名称", trigger: "blur" }],
  host: [{ required: true, message: "请输入主机地址", trigger: "blur" }],
  username: [{ required: true, message: "请输入登录用户", trigger: "blur" }],
  private_key_path: [
    {
      validator: (_rule, value, callback) => {
        if (form.auth_method === "key" && !String(value || "").trim()) {
          callback(new Error("使用密钥认证时必须填写私钥路径"));
          return;
        }
        callback();
      },
      trigger: "blur",
    },
  ],
};

const dialogTitle = computed(() =>
  editingId.value === null ? "添加服务器" : "编辑服务器",
);

/** 每个服务器被哪些项目引用——删除前用来判断影响面。 */
function projectsUsing(serverId: number): Project[] {
  return projects.value.filter((p) => p.server_id === serverId);
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const [serverList, projectList] = await Promise.all([
      listServers(),
      listProjects(),
    ]);
    servers.value = serverList;
    projects.value = projectList;
  } catch (err) {
    error.value = err instanceof Error ? err.message : "服务器列表加载失败";
  } finally {
    loading.value = false;
  }
}

function openCreate() {
  editingId.value = null;
  Object.assign(form, emptyServer());
  dialogVisible.value = true;
}

function openEdit(server: Server) {
  editingId.value = server.id;
  Object.assign(form, {
    name: server.name,
    group_name: server.group_name,
    host: server.host,
    port: server.port,
    username: server.username,
    auth_method: server.auth_method,
    private_key_path: server.private_key_path,
    // 凭据留空表示「不修改」：后端把 undefined / null 当作保持原值
    private_key_passphrase: null,
    password: null,
    deploy_path: server.deploy_path,
    enabled: server.enabled,
  });
  dialogVisible.value = true;
}

async function save() {
  const formInstance = formRef.value;
  if (!formInstance) {
    return;
  }
  const valid = await formInstance.validate().catch(() => false);
  if (!valid) {
    return;
  }

  saving.value = true;
  try {
    const payload: ServerPayload = {
      ...form,
      group_name: form.group_name?.trim() || null,
      private_key_path:
        form.auth_method === "key" ? form.private_key_path?.trim() || null : null,
      deploy_path: form.deploy_path?.trim() || null,
      // 空串交给后端清除，null 表示保持原值
      private_key_passphrase: form.private_key_passphrase
        ? form.private_key_passphrase
        : null,
      password: form.password ? form.password : null,
    };

    if (editingId.value === null) {
      await createServer(payload);
      ElMessage.success("服务器已创建");
    } else {
      await updateServer(editingId.value, payload);
      ElMessage.success("服务器已更新");
    }
    dialogVisible.value = false;
    await load();
  } catch (err) {
    ElMessage.error(err instanceof Error ? err.message : "保存失败");
  } finally {
    saving.value = false;
  }
}

async function runTest(server: Server) {
  testing.value = server.id;
  delete testResults[server.id];
  try {
    const result = await testServer(server.id);
    testResults[server.id] = result.ok
      ? "连接正常"
      : result.message || "连接失败";
    ElMessage[result.ok ? "success" : "error"](
      result.ok ? `${server.name} 连接正常` : `${server.name}：${result.message}`,
    );
  } catch (err) {
    const message = err instanceof Error ? err.message : "连接测试失败";
    testResults[server.id] = message;
    ElMessage.error(message);
  } finally {
    testing.value = null;
  }
}

async function remove(server: Server) {
  const used = projectsUsing(server.id);
  // 后端会拒绝删除被引用的服务器，这里先在前端说清楚是哪几个项目
  if (used.length > 0) {
    ElMessageBox.alert(
      `仍有 ${used.length} 个项目在使用「${server.name}」：${used
        .map((p) => p.name)
        .join("、")}。请先把它们改到其他服务器。`,
      "无法删除",
      { type: "warning", confirmButtonText: "知道了" },
    );
    return;
  }

  try {
    await ElMessageBox.confirm(
      `删除服务器「${server.name}」？此操作不可撤销。`,
      "删除确认",
      { type: "warning", confirmButtonText: "删除", cancelButtonText: "取消" },
    );
  } catch {
    return;
  }

  try {
    await deleteServer(server.id);
    ElMessage.success("服务器已删除");
    await load();
  } catch (err) {
    ElMessage.error(err instanceof Error ? err.message : "删除失败");
  }
}

onMounted(load);
</script>

<template>
    <div>
        <PageIntro
            eyebrow="Workspace / Servers"
            title="服务器"
            description="SSH 连接集中在这里维护，项目部署时直接选用。凭据保存在本地数据库，不会下发到浏览器。"
        >
            <template #actions>
                <el-button :loading="loading" @click="load">刷新</el-button>
                <el-button type="primary" @click="openCreate">添加服务器</el-button>
            </template>
        </PageIntro>

        <section class="content">
            <div v-if="error" class="error-bar">
                <span>{{ error }}</span>
                <el-button text @click="load">重试</el-button>
            </div>

            <div v-if="loading" class="state">正在读取服务器配置...</div>

            <div v-else-if="servers.length === 0" class="empty">
                <strong>还没有服务器</strong>
                <p>添加一台服务器后，就可以在项目里选择它作为部署目标。</p>
                <el-button type="primary" @click="openCreate">添加服务器</el-button>
            </div>

            <div v-else class="grid">
                <article v-for="server in servers" :key="server.id" class="card">
                    <div class="card-h">
                        <span class="card-n">{{ server.name }}</span>
                        <span
                            class="chip"
                            :class="server.enabled ? 'chip--ok' : 'chip--neu'"
                        >
                            {{ server.enabled ? "已启用" : "已停用" }}
                        </span>
                    </div>

                    <div class="card-kind">
                        <span class="cap">{{ server.group_name || "未分组" }}</span>
                    </div>

                    <dl class="fields">
                        <div>
                            <dt>SSH 地址</dt>
                            <dd>{{ server.username }}@{{ server.host }}:{{ server.port }}</dd>
                        </div>
                        <div>
                            <dt>认证方式</dt>
                            <dd>
                                <span class="auth">
                                    <i :class="server.auth_method === 'key' ? 'is-key' : 'is-pw'" />
                                    {{ server.auth_method === "key" ? "SSH 密钥" : "密码" }}
                                </span>
                                <span v-if="server.auth_method === 'password'" class="auth-ph">
                                    ••••••••
                                </span>
                            </dd>
                        </div>
                        <div>
                            <dt>{{ server.auth_method === "key" ? "私钥路径" : "凭据" }}</dt>
                            <dd>
                                {{
                                    server.auth_method === "key"
                                        ? server.private_key_path || "—"
                                        : server.password_set
                                            ? "密码已保存"
                                            : "未设置密码"
                                }}
                            </dd>
                        </div>
                        <div>
                            <dt>被引用的项目</dt>
                            <dd :class="{ 'is-nil': server.project_count === 0 }">
                                {{
                                    server.project_count > 0
                                        ? `${server.project_count} 个`
                                        : "未被引用"
                                }}
                            </dd>
                        </div>
                    </dl>

                    <div class="card-f">
                        <span v-if="testResults[server.id]" class="test-out">
                            {{ testResults[server.id] }}
                        </span>
                        <span v-else class="test-out is-muted">
                            {{ server.deploy_path || "未指定部署目录" }}
                        </span>
                        <el-button
                            size="small"
                            :loading="testing === server.id"
                            @click="runTest(server)"
                        >
                            测试连接
                        </el-button>
                        <el-button size="small" @click="openEdit(server)">编辑</el-button>
                        <el-button size="small" @click="remove(server)">删除</el-button>
                    </div>
                </article>
            </div>

            <div class="panel note">
                <p>
                    服务器需要先被部署服务信任：<code class="mono">ssh-keyscan -p &lt;端口&gt; &lt;主机&gt; &gt;&gt; ~/.ssh/known_hosts</code><br />
                    主机公钥校验失败会直接中止部署并在日志里说明原因。密码登录同样要过这一步。
                </p>
            </div>
        </section>

        <ServerForm
            v-model:visible="dialogVisible"
            v-model:form="form"
            :title="dialogTitle"
            :rules="rules"
            :saving="saving"
            :editing="editingId !== null"
            @submit="save"
        />    </div>
</template>

<style scoped>
.content { padding: 20px 24px 44px; }

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

.state,
.empty {
    display: flex;
    min-height: 160px;
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
.auth { display: inline-flex; align-items: center; gap: 4px; }
.auth i { width: 5px; height: 5px; border-radius: 50%; }
.auth i.is-key { background: var(--tag); }
.auth i.is-pw { background: var(--run); }
.auth-ph { margin-left: 6px; color: var(--tx4); letter-spacing: 0.08em; }

.card-f { display: flex; align-items: center; gap: 6px; padding: 9px 13px; }

.test-out {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--tx3);
    font-family: var(--font-mono);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.test-out.is-muted { color: var(--tx4); }

.note { margin-top: 12px; padding: 13px 15px; }
.note p { margin: 0; color: var(--tx3); font-size: 12px; line-height: 1.7; }
.note code { color: var(--tx2); }

@media (max-width: 760px) {
    .content { padding: 14px 12px 34px; }
}
</style>
