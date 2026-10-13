<script setup lang="ts">
import { computed } from "vue";

/**
 * 环形资源指标。
 *
 * `value` 为 null 表示连不上、采不到数，此时渲染虚线空环加破折号——
 * 显示 0% 会被读成「资源很空闲」，比不显示更危险。
 */
const props = withDefaults(
  defineProps<{
    value: number | null;
    label: string;
    size?: number;
    stroke?: number;
  }>(),
  { size: 76, stroke: 6 },
);

const radius = computed(() => (props.size - props.stroke) / 2);
const circumference = computed(() => 2 * Math.PI * radius.value);

const hasValue = computed(
  () => props.value !== null && Number.isFinite(props.value),
);

/** 阈值：≥90 紧张、≥75 偏高，其余正常。 */
const severity = computed(() => {
  if (!hasValue.value) {
    return "m-off";
  }
  const v = props.value as number;
  if (v >= 90) {
    return "m-crit";
  }
  if (v >= 75) {
    return "m-warn";
  }
  return "m-ok";
});

const offset = computed(() =>
  hasValue.value
    ? circumference.value * (1 - (props.value as number) / 100)
    : circumference.value,
);

const display = computed(() =>
  hasValue.value ? Math.round(props.value as number) : null,
);

const numberSize = computed(() => (props.size >= 76 ? 20 : 17));
</script>

<template>
    <div
        class="ring"
        :class="severity"
        :style="{ width: `${size}px`, height: `${size}px` }"
        role="img"
        :aria-label="`${label} 使用率 ${display === null ? '未知' : `${display}%`}`"
    >
        <svg :width="size" :height="size" aria-hidden="true">
            <circle
                class="ring-track"
                :cx="size / 2"
                :cy="size / 2"
                :r="radius"
                :stroke-width="stroke"
                :stroke-dasharray="hasValue ? undefined : '3 4'"
            />
            <circle
                v-if="hasValue"
                class="ring-val"
                :cx="size / 2"
                :cy="size / 2"
                :r="radius"
                :stroke-width="stroke"
                :stroke-dasharray="circumference"
                :stroke-dashoffset="offset"
            />
        </svg>
        <span class="ring-in">
            <span class="ring-n" :style="{ fontSize: `${numberSize}px` }">
                <template v-if="display === null">—</template>
                <template v-else>{{ display }}<i>%</i></template>
            </span>
            <span class="ring-u">{{ label }}</span>
        </span>
    </div>
</template>

<style scoped>
.ring { position: relative; display: grid; place-items: center; flex-shrink: 0; }
.ring svg { transform: rotate(-90deg); }
.ring-track { fill: none; stroke: var(--sf3); }
.ring-val { fill: none; stroke-linecap: round; transition: stroke-dashoffset 400ms ease; }

.ring-in {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    line-height: 1;
}

.ring-n {
    font-family: var(--font-mono);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.03em;
}

.ring-n i { font-size: 0.55em; font-style: normal; font-weight: 500; }

.ring-u {
    margin-top: 2px;
    color: var(--tx4);
    font-size: 9px;
    letter-spacing: 0.06em;
}

.m-ok .ring-val { stroke: var(--ok-fill); }
.m-warn .ring-val { stroke: var(--run-fill); }
.m-crit .ring-val { stroke: var(--bad-fill); }
.m-ok .ring-n { color: var(--ok); }
.m-warn .ring-n { color: var(--run); }
.m-crit .ring-n { color: var(--bad); }
.m-off .ring-val { stroke: var(--neu-fill); }
.m-off .ring-n { color: var(--tx3); }
</style>
