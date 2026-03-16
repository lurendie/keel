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
        <div>
            <div class="page-eyebrow">{{ eyebrow }}</div>
            <h1>{{ title }}</h1>
            <p v-if="description">{{ description }}</p>
        </div>
        <div class="intro-slot">
            <slot name="actions" />
        </div>
    </div>
</template>

<style scoped>
.page-intro {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    padding: 34px 36px 26px;
    background: #ffffff;
    border-bottom: 1px solid #d9ded9;
}

.page-eyebrow {
    color: #879189;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.16em;
    text-transform: uppercase;
}

h1 {
    margin: 8px 0 0;
    color: #202723;
    font-family: Georgia, serif;
    font-size: clamp(27px, 3vw, 38px);
    font-weight: 500;
    letter-spacing: 0;
    line-height: 1;
}

p {
    max-width: 620px;
    margin: 12px 0 0;
    color: #6e786f;
    font-size: 13px;
    line-height: 1.55;
}

.intro-slot {
    flex-shrink: 0;
}

@media (max-width: 720px) {
    .page-intro {
        align-items: flex-start;
        flex-direction: column;
        padding: 28px 18px 22px;
    }
}
</style>
