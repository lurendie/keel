<script setup lang="ts">
withDefaults(
  defineProps<{
    eyebrow: string;
    title: string;
    description?: string;
  }>(),
  {
    description: "",
  },
);
</script>

<template>
    <div class="page-intro">
        <h1>{{ title }}</h1>
        <div class="intro-slot">
            <slot name="actions" />
        </div>
    </div>
</template>

<style scoped>
.page-intro {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 24px 0;
}

h1 {
    margin: 0;
    color: var(--tx);
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.01em;
    line-height: 1.2;
}

.intro-slot {
    display: flex;
    align-items: center;
    flex-shrink: 0;
}

@media (max-width: 760px) {
    .page-intro {
        gap: 10px;
        padding: 14px 12px 0;
    }

    h1 { font-size: 18px; }
}
</style>
