<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { Check, CopyDocument, Key, Lock, Monitor, Refresh } from "@element-plus/icons-vue";
import { ElMessage, type FormInstance, type FormRules } from "element-plus";
import PageIntro from "../components/PageIntro.vue";
import {
  changePassword,
  getWebhookSecret,
  updateWebhookSecret,
} from "../api/api";

const secretFormRef = ref<FormInstance>();
const passwordFormRef = ref<FormInstance>();
const loading = ref(true);
const savingSecret = ref(false);
const changingPassword = ref(false);
const copied = ref(false);

const secretForm = reactive({
  webhook_secret: "",
});

const passwordForm = reactive({
  old_password: "",
  new_password: "",
  confirm_password: "",
});

const secretRules: FormRules = {
  webhook_secret: [{ max: 240, message: "Secret 长度不能超过 240 个字符", trigger: "blur" }],
};

const passwordRules: FormRules = {
  old_password: [{ required: true, message: "请输入当前密码", trigger: "blur" }],
  new_password: [
    { required: true, message: "请输入新密码", trigger: "blur" },
    { min: 8, message: "新密码至少 8 位", trigger: "blur" },
  ],
  confirm_password: [
    { required: true, message: "请再次输入新密码", trigger: "blur" },
    {
      validator: (_rule, value, callback) => {
        if (value !== passwordForm.new_password) {
          callback(new Error("两次输入的密码不一致"));
          return;
        }
        callback();
      },
      trigger: "blur",
    },
  ],
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const result = await getWebhookSecret();
    secretForm.webhook_secret = result.webhook_secret;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "设置加载失败");
  } finally {
    loading.value = false;
  }
};

const saveSecret = async () => {
  if (!secretFormRef.value) return;

  try {
    await secretFormRef.value.validate();
    savingSecret.value = true;
    await updateWebhookSecret(secretForm.webhook_secret.trim());
    ElMessage.success("Webhook Secret 已保存");
  } catch (error) {
    if (error instanceof Error) {
      ElMessage.error(error.message);
    }
  } finally {
    savingSecret.value = false;
  }
};

const updatePassword = async () => {
  if (!passwordFormRef.value) return;

  try {
    await passwordFormRef.value.validate();
    changingPassword.value = true;
    await changePassword(passwordForm.old_password, passwordForm.new_password);
    Object.assign(passwordForm, {
      old_password: "",
      new_password: "",
      confirm_password: "",
    });
    ElMessage.success("密码已更新");
  } catch (error) {
    if (error instanceof Error) {
      ElMessage.error(error.message);
    }
  } finally {
    changingPassword.value = false;
  }
};

const copySecret = async () => {
  if (!secretForm.webhook_secret) {
    ElMessage.info("当前没有配置 Secret");
    return;
  }

  try {
    await navigator.clipboard.writeText(secretForm.webhook_secret);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1600);
  } catch {
    ElMessage.error("复制失败，请手动选择文本");
  }
};

onMounted(loadSettings);
</script>

<template>
    <div class="settings-page">
        <PageIntro
            eyebrow="Workspace / System settings"
            title="系统设置"
            description="管理 webhook 的签名密钥和当前账户密码。部署服务器密钥属于项目配置的一部分。"
        >
            <template #actions>
                <el-button class="quiet-button" :loading="loading" @click="loadSettings">
                    <el-icon><Refresh /></el-icon>
                    重新读取
                </el-button>
            </template>
        </PageIntro>

        <section class="settings-content">
            <div class="settings-grid">
                <div class="settings-card settings-card-wide">
                    <div class="card-heading">
                        <div class="card-icon"><Monitor /></div>
                        <div>
                            <span class="card-kicker">Inbound protection</span>
                            <h2>Webhook 签名</h2>
                        </div>
                    </div>
                    <p class="card-description">
                        GitHub、Gitea 和 CI 回调使用同一个 Secret。CI 回调未配置 Secret 时会被直接拒绝。
                    </p>

                    <el-form ref="secretFormRef" :model="secretForm" :rules="secretRules" label-position="top">
                        <el-form-item label="Webhook Secret" prop="webhook_secret">
                            <el-input
                                v-model="secretForm.webhook_secret"
                                type="password"
                                show-password
                                placeholder="输入用于校验请求的密钥"
                            >
                                <template #prefix><el-icon><Key /></el-icon></template>
                                <template #append>
                                    <el-button :aria-label="copied ? '已复制' : '复制 Secret'" @click="copySecret">
                                        <el-icon><Check v-if="copied" /><CopyDocument v-else /></el-icon>
                                    </el-button>
                                </template>
                            </el-input>
                        </el-form-item>
                        <div class="card-actions">
                            <el-button type="primary" :loading="savingSecret" @click="saveSecret">保存 Secret</el-button>
                        </div>
                    </el-form>

                    <div class="endpoint-note">
                        <span class="endpoint-label">Ingress endpoints</span>
                        <div class="endpoint-codes">
                            <code>POST /webhook</code>
                            <code>POST /api/deploy/callback</code>
                        </div>
                    </div>
                </div>

                <div class="settings-card">
                    <div class="card-heading">
                        <div class="card-icon"><Lock /></div>
                        <div>
                            <span class="card-kicker">Account security</span>
                            <h2>修改密码</h2>
                        </div>
                    </div>
                    <p class="card-description">建议使用专用密码，不要与服务器登录凭据重复。</p>

                    <el-form ref="passwordFormRef" :model="passwordForm" :rules="passwordRules" label-position="top">
                        <el-form-item label="当前密码" prop="old_password">
                            <el-input v-model="passwordForm.old_password" type="password" show-password />
                        </el-form-item>
                        <el-form-item label="新密码" prop="new_password">
                            <el-input v-model="passwordForm.new_password" type="password" show-password />
                        </el-form-item>
                        <el-form-item label="确认新密码" prop="confirm_password">
                            <el-input v-model="passwordForm.confirm_password" type="password" show-password @keyup.enter="updatePassword" />
                        </el-form-item>
                        <div class="card-actions">
                            <el-button type="primary" :loading="changingPassword" @click="updatePassword">更新密码</el-button>
                        </div>
                    </el-form>
                </div>
            </div>

            <div class="security-callout">
                <span class="callout-mark">!</span>
                <div>
                    <strong>部署安全提示</strong>
                    <p>SSH 私钥建议使用专用 deploy 用户和最小权限。不要把私钥内容写进项目配置或提交到仓库。</p>
                </div>
            </div>
        </section>
    </div>
</template>

<style scoped>
.settings-page {
    min-height: 100%;
}

.settings-content {
    padding: 28px 36px 44px;
}

.settings-grid {
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(320px, 0.85fr);
    gap: 10px;
}

.settings-card {
    padding: 24px;
    background: #ffffff;
    border: 1px solid #d9ded9;
}

.settings-card-wide {
    min-height: 430px;
}

.card-heading {
    display: flex;
    align-items: center;
    gap: 12px;
}

.card-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    color: #404040;
    background: #f0f0f0;
}

.card-icon :deep(svg) {
    width: 18px;
}

.card-kicker {
    color: #879189;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.14em;
    text-transform: uppercase;
}

.card-heading h2 {
    margin: 5px 0 0;
    color: #354139;
    font-family: Georgia, serif;
    font-size: 21px;
    font-weight: 500;
}

.card-description {
    max-width: 610px;
    margin: 20px 0 22px;
    color: #78837b;
    font-size: 12px;
    line-height: 1.6;
}

.settings-card :deep(.el-form-item) {
    margin-bottom: 16px;
}

.settings-card :deep(.el-form-item__label) {
    height: auto;
    margin-bottom: 6px;
    padding: 0;
    color: #66736a;
    font-size: 11px;
    line-height: 1.2;
}

.card-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 6px;
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

.endpoint-note {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 34px;
    padding-top: 14px;
    border-top: 1px solid #e1e5e1;
}

.endpoint-label {
    color: #879189;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.1em;
    text-transform: uppercase;
}

.endpoint-codes {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
}

code {
    padding: 5px 8px;
    color: #404040;
    background: #f0f0f0;
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: 11px;
}

.security-callout {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-top: 10px;
    padding: 17px 19px;
    color: #6e786f;
    background: #f7efd5;
    border: 1px solid #e9d99e;
}

.callout-mark {
    display: grid;
    flex-shrink: 0;
    width: 21px;
    height: 21px;
    place-items: center;
    color: #8a6d2f;
    border: 1px solid #cfae58;
    border-radius: 50%;
    font-size: 12px;
    font-weight: 800;
}

.security-callout strong {
    color: #6f5c29;
    font-size: 12px;
}

.security-callout p {
    margin: 5px 0 0;
    color: #8a7642;
    font-size: 11px;
    line-height: 1.5;
}

@media (max-width: 920px) {
    .settings-content {
        padding: 24px;
    }

    .settings-grid {
        grid-template-columns: 1fr;
    }

    .settings-card-wide {
        min-height: auto;
    }
}

@media (max-width: 720px) {
    .settings-content {
        padding: 20px 18px 32px;
    }

    .settings-card {
        padding: 20px;
    }

    .endpoint-note {
        align-items: flex-start;
        gap: 10px;
        flex-direction: column;
    }
}
</style>
