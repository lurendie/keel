<template>
    <div class="login-container">
        <el-card class="login-card">
            <div class="login-header">
                <h2>Keel</h2>
                <p>Release delivery control</p>
            </div>

            <el-form
                ref="loginFormRef"
                :model="loginForm"
                :rules="loginRules"
                label-width="0"
                class="login-form"
            >
                <el-form-item prop="username">
                    <el-input
                        v-model="loginForm.username"
                        placeholder="请输入用户名"
                        :prefix-icon="User"
                        size="large"
                    />
                </el-form-item>

                <el-form-item prop="password">
                    <el-input
                        v-model="loginForm.password"
                        type="password"
                        placeholder="请输入密码"
                        :prefix-icon="Lock"
                        size="large"
                        show-password
                        @keyup.enter="handleLogin"
                    />
                </el-form-item>

                <el-form-item>
                    <el-button type="primary" size="large" class="login-btn" :loading="loading" @click="handleLogin">
                        登录
                    </el-button>
                </el-form-item>
            </el-form>
        </el-card>
    </div>
</template>

<script setup lang="ts">
import { reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ElMessage, type FormInstance } from "element-plus";
import { Lock, User } from "@element-plus/icons-vue";
import { login, setToken } from "../api/api";

const router = useRouter();
const route = useRoute();

const loginForm = reactive({
  username: "",
  password: "",
});

const loginRules = {
  username: [
    { required: true, message: "请输入用户名", trigger: "blur" },
    { min: 3, max: 32, message: "用户名长度在 3 到 32 个字符", trigger: "blur" },
  ],
  password: [
    { required: true, message: "请输入密码", trigger: "blur" },
    { min: 6, max: 128, message: "密码长度至少 6 位", trigger: "blur" },
  ],
};

const loginFormRef = ref<FormInstance>();
const loading = ref(false);

const handleLogin = async () => {
  if (!loginFormRef.value) {
    return;
  }

  try {
    await loginFormRef.value.validate();
    loading.value = true;

    const result = await login(loginForm.username, loginForm.password);
    setToken(result.token);
    ElMessage.success("登录成功");

    const redirect = typeof route.query.redirect === "string" ? route.query.redirect : "/";
    await router.replace(redirect);
  } catch (error) {
    if (error instanceof Error) {
      ElMessage.error(error.message);
    }
  } finally {
    loading.value = false;
  }
};
</script>

<style scoped>

    /* 整个页面铺满，背景渐变色 */
    .login-container {
        display: flex;
        justify-content: center;
        align-items: center;
        height: 100vh;
        /* background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); */
        margin: 0;
        padding: 0;
    }

    /* 卡片样式：居中、带阴影、圆角 */
    .login-card {
        width: 400px;
        padding: 20px 30px 30px;
        border-radius: 16px;
        box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
        background: rgba(255, 255, 255, 0.95);
        backdrop-filter: blur(10px);
    }

    /* 头部标题 */
    .login-header {
        text-align: center;
        margin-bottom: 30px;
    }

    .login-header h2 {
        font-size: 28px;
        color: #333;
        margin: 0 0 6px;
    }

    .login-header p {
        color: #888;
        font-size: 14px;
        margin: 0;
    }

    /* 表单样式：去除 label 宽度，输入框撑满 */
    .login-form .el-form-item {
        margin-bottom: 22px;
    }

    /* 登录按钮全宽 */
    .login-btn {
        width: 100%;
    }

</style>
