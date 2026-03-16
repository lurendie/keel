<script setup lang="ts">
withDefaults(
  defineProps<{
    status: string;
    label?: string;
  }>(),
  {
    label: "",
  },
);

const statusClass = (status: string) => {
  if (["success", "healthy", "enabled"].includes(status.toLowerCase())) {
    return "is-success";
  }
  if (["failed", "error", "disabled"].includes(status.toLowerCase())) {
    return "is-danger";
  }
  if (["running", "queued", "pending"].includes(status.toLowerCase())) {
    return "is-running";
  }
  return "is-neutral";
};
</script>

<template>
    <span class="status-pill" :class="statusClass(status)">
        <span class="status-dot"></span>
        {{ label || status }}
    </span>
</template>

<style scoped>
.status-pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 24px;
    padding: 0 9px;
    border: 1px solid transparent;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
}

.status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
}

.is-success {
    color: #35688f;
    background: #eaf2f8;
    border-color: #cfe0ec;
}

.is-danger {
    color: #a94d42;
    background: #f8e6e1;
    border-color: #edc7bd;
}

.is-running {
    color: #8a6d2f;
    background: #f7efd5;
    border-color: #e9d99e;
}

.is-neutral {
    color: #67736b;
    background: #f2f2f2;
    border-color: #dedede;
}
</style>
