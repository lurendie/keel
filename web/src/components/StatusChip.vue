<script setup lang="ts">
import { computed } from "vue";
import { statusLabel as labelFor, statusVariant } from "../constants/deploy";

const props = defineProps<{
  status: string;
  /** 覆盖默认措辞，例如「已启用」/「已停用」。 */
  label?: string;
  variant?: string;
}>();

const variant = computed(() => props.variant ?? statusVariant(props.status));
const text = computed(() => props.label ?? labelFor(props.status));
</script>

<template>
    <span class="chip" :class="`chip--${variant}`">{{ text }}</span>
</template>

<style scoped>
.chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 21px;
    padding: 0 7px;
    border: 1px solid;
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
}

.chip::before {
    width: 5px;
    height: 5px;
    flex-shrink: 0;
    content: "";
    border-radius: 50%;
    background: currentColor;
}

.chip--ok { color: var(--ok); background: var(--ok-bg); border-color: var(--ok-bd); }
.chip--run { color: var(--run); background: var(--run-bg); border-color: var(--run-bd); }
.chip--bad { color: var(--bad); background: var(--bad-bg); border-color: var(--bad-bd); }
.chip--neu { color: var(--neu); background: var(--neu-bg); border-color: var(--neu-bd); }
.chip--tag { color: var(--tag); background: var(--tag-bg); border-color: var(--tag-bd); }

.chip--run::before { animation: keel-blink 1.3s steps(1, end) infinite; }
</style>
