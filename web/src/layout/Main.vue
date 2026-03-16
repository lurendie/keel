<template>
    <el-main class="layout-main">
        <router-view />
    </el-main>
</template>

<style lang="css" scoped>
.layout-main {
    width: 100%;
    height: 100%;
    min-width: 0;
    padding: 0;
    overflow-y: auto;
    background: #ffffff;
}

@media (max-width: 720px) {
    .layout-main {
        width: calc(100% - 76px);
        margin-left: 76px;
    }
}
</style>
