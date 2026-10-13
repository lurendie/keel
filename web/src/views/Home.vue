<script setup lang="ts">
import Aside from "../layout/Aside.vue";
import Header from "../layout/Header.vue";
import Main from "../layout/Main.vue";
</script>

<template>
    <el-container class="app-shell">
        <Aside />
        <el-container class="app-column" direction="vertical">
            <Header />
            <Main />
        </el-container>
    </el-container>
</template>

<style lang="css" scoped>
.app-shell {
    display: flex;
    flex-direction: row;
    height: 100vh;
    min-height: 100vh;
    overflow: hidden;
    background: var(--pg);
}

.app-column {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--pg);
}

@media (max-width: 760px) {
    .app-shell {
        height: auto;
        min-height: 100vh;
        flex-direction: column;
        overflow: visible;
    }

    .app-column {
        min-height: calc(100vh - 50px);
    }
}
</style>
