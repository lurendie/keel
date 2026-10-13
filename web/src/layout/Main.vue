<template>
    <el-main class="layout-main">
        <router-view />
    </el-main>
</template>

<style lang="css" scoped>
.layout-main {
    flex: 1;
    width: 100%;
    height: auto;
    min-width: 0;
    min-height: 0;
    padding: 0;
    overflow-y: auto;
    background: var(--pg);
}
</style>
