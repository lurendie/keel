<script setup lang="ts">
import Aside from "../layout/Aside.vue";
import Footer from "../layout/Footer.vue";
import Header from "../layout/Header.vue";
import Main from "../layout/Main.vue";
</script>

<template>
    <el-container class="app-shell" direction="vertical">
        <Header />
        <el-container class="app-body" direction="horizontal">
            <Aside />
            <Main />
        </el-container>
        <Footer />
    </el-container>
</template>

<style lang="css" scoped>
.app-shell {
    height: 100vh;
    min-height: 100vh;
    overflow: hidden;
    background: #ffffff;
}

.app-body {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: #ffffff;
}
</style>
