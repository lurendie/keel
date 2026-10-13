<script setup lang="ts">
import { ref } from "vue";
import type { FormInstance, FormRules } from "element-plus";
import type { ServerPayload } from "../api/api";

const visible = defineModel<boolean>("visible", { required: true });
const form = defineModel<ServerPayload>("form", { required: true });

const props = withDefaults(
  defineProps<{
    title: string;
    rules: FormRules<ServerPayload>;
    saving?: boolean;
    editing?: boolean;
  }>(),
  { saving: false, editing: false },
);

const emit = defineEmits<{ submit: [] }>();

const showPassword = ref(false);
const internalFormRef = ref<FormInstance>();

defineExpose({
  validate: () =>
    internalFormRef.value ? internalFormRef.value.validate() : Promise.resolve(false),
});

function submit() {
  emit("submit");
}

function onClosed() {
  showPassword.value = false;
}
</script>

<template>
    <el-dialog
        v-model="visible"
        :title="title"
        width="560px"
        destroy-on-close
        @closed="onClosed"
    >
        <el-form
            ref="internalFormRef"
            :model="form"
            :rules="props.rules"
            label-position="top"
        >            <div class="group">
                <div class="group-t"><span class="cap">01 · 标识</span></div>
                <div class="two">
                    <el-form-item label="名称" prop="name">
                        <el-input v-model="form.name" placeholder="生产 Web 节点" />
                    </el-form-item>
                    <el-form-item label="分组（可选）">
                        <el-input v-model="form.group_name" placeholder="production" />
                    </el-form-item>
                </div>
            </div>

            <div class="group">
                <div class="group-t"><span class="cap">02 · 连接</span></div>
                <div class="two">
                    <el-form-item label="主机" prop="host">
                        <el-input v-model="form.host" placeholder="10.0.0.8" />
                    </el-form-item>
                    <el-form-item label="端口">
                        <el-input v-model.number="form.port" />
                    </el-form-item>
                </div>
                <el-form-item label="登录用户" prop="username">
                    <el-input v-model="form.username" placeholder="deploy" />
                </el-form-item>
                <el-form-item label="部署目录（可选）">
                    <el-input
                        v-model="form.deploy_path"
                        placeholder="/srv/apps/demo"
                    />
                    <div class="hint">
                        用于采集磁盘用量；留空则统计根分区。
                    </div>
                </el-form-item>
            </div>

            <div class="group">
                <div class="group-t"><span class="cap">03 · 认证方式</span></div>

                <el-radio-group v-model="form.auth_method" class="auth-seg">
                    <el-radio-button value="key">SSH 密钥</el-radio-button>
                    <el-radio-button value="password">密码</el-radio-button>
                </el-radio-group>

                <template v-if="form.auth_method === 'key'">
                    <el-form-item label="私钥路径" prop="private_key_path">
                        <el-input
                            v-model="form.private_key_path"
                            placeholder="~/.ssh/id_ed25519"
                        />
                        <div class="hint">
                            部署服务所在机器上的路径。密钥内容不会上传，只记录路径。
                        </div>
                    </el-form-item>
                    <el-form-item label="私钥密码（可选）">
                        <el-input
                            v-model="form.private_key_passphrase"
                            type="password"
                            show-password
                            :placeholder="
                                props.editing ? '留空表示不修改' : '留空表示私钥无密码'
                            "
                        />
                    </el-form-item>
                </template>

                <template v-else>
                    <el-form-item label="登录密码">
                        <el-input
                            v-model="form.password"
                            :type="showPassword ? 'text' : 'password'"
                            :placeholder="
                                props.editing ? '留空表示不修改' : '输入服务器登录密码'
                            "
                        >
                            <template #append>
                                <el-button @click="showPassword = !showPassword">
                                    {{ showPassword ? "隐藏" : "显示" }}
                                </el-button>
                            </template>
                        </el-input>
                        <div class="hint">
                            密码保存在本地数据库。建议为该账号配置 sudo 白名单，或改用密钥登录。
                        </div>
                    </el-form-item>
                </template>
            </div>

            <el-form-item>
                <el-checkbox v-model="form.enabled">启用（参与自动部署与资源采集）</el-checkbox>
            </el-form-item>
        </el-form>

        <template #footer>
            <el-button @click="visible = false">取消</el-button>
            <el-button type="primary" :loading="props.saving" @click="submit">
                保存服务器
            </el-button>
        </template>
    </el-dialog>
</template>

<style scoped>
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
    line-height: 1.45;
}

.auth-seg { margin-bottom: 14px; }

@media (max-width: 620px) {
    .two { grid-template-columns: 1fr; }
}
</style>
