<template>
    <el-footer class="layout-footer">
        <span class="mono">Keel Release Delivery</span>
        <span>Webhook ingress · Artifact delivery · SSH rollout</span>
    </el-footer>
</template>

<style lang="css" scoped>
.layout-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 38px;
    padding: 0 24px;
    color: var(--tx3);
    background: var(--sf);
    border-top: 1px solid var(--bd);
    font-size: 11px;
    letter-spacing: 0.04em;
}

@media (max-width: 640px) {
    .layout-footer {
        height: auto;
        gap: 6px;
        align-items: flex-start;
        flex-direction: column;
        padding: 12px 16px 12px 88px;
    }
}
</style>
