<template>
    <el-footer class="layout-footer">
        <span>Keel Release Delivery</span>
        <span>Webhook ingress · Artifact delivery · SSH rollout</span>
    </el-footer>
</template>

<style lang="css" scoped>
.layout-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 38px;
    padding: 0 28px;
    color: #7a817b;
    background: #ffffff;
    border-top: 1px solid #d9ded9;
    font-size: 11px;
    letter-spacing: 0.04em;
}

@media (max-width: 640px) {
    .layout-footer {
        height: auto;
        gap: 6px;
        align-items: flex-start;
        flex-direction: column;
        padding: 12px 18px 12px 94px;
    }
}
</style>
