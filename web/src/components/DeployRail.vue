<script setup lang="ts">
import { buildRail, type RailStage } from "../constants/deploy";
import type { DeployMode } from "../api/api";

const props = withDefaults(
  defineProps<{
    status: string;
    mode: DeployMode;
    nowStage?: number | null;
    failedStage?: number | null;
    stageDurations?: (string | null)[];
    halted?: boolean;
    /** 失败时展示的原始错误，紧贴断点下方。 */
    errorDetail?: string | null;
    /** 运行中：显示已耗时与「日志结束后落库」的说明。 */
    runningSince?: string | null;
  }>(),
  {
    nowStage: null,
    failedStage: null,
    stageDurations: () => [],
    halted: false,
    errorDetail: null,
    runningSince: null,
  },
);

const stages: RailStage[] = buildRail({
  status: props.status,
  mode: props.mode,
  nowStage: props.nowStage,
  failedStage: props.failedStage,
  stageDurations: props.stageDurations,
  halted: props.halted,
});

/** 失败断点：那一段的连接线画成红色虚线，表示管线在此中断。 */
function isBroken(index: number): boolean {
  return (
    props.status === "failed" &&
    props.failedStage != null &&
    index + 1 === props.failedStage
  );
}

const failedName =
  props.failedStage != null ? stages[props.failedStage]?.name : null;
</script>

<template>
    <div>
        <div class="rail">
            <div
                v-for="(stage, index) in stages"
                :key="stage.name"
                class="rail-st"
                :class="stage.state"
            >
                <div class="rail-node">
                    <span class="rail-dot">
                        <svg
                            v-if="stage.state === 'is-done'"
                            viewBox="0 0 16 16"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2.4"
                            aria-hidden="true"
                        >
                            <path d="m3.4 8.4 3 3 6.2-6.8" stroke-linecap="round" stroke-linejoin="round" />
                        </svg>
                        <svg
                            v-else-if="stage.state === 'is-bad'"
                            viewBox="0 0 16 16"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2.4"
                            aria-hidden="true"
                        >
                            <path d="M4.4 4.4l7.2 7.2M11.6 4.4l-7.2 7.2" stroke-linecap="round" />
                        </svg>
                        <svg
                            v-else-if="stage.state === 'is-now'"
                            viewBox="0 0 16 16"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            aria-hidden="true"
                        >
                            <path d="M8 1.8a6.2 6.2 0 1 1-6.2 6.2" stroke-linecap="round" />
                        </svg>
                        <svg
                            v-else
                            viewBox="0 0 16 16"
                            fill="currentColor"
                            aria-hidden="true"
                        >
                            <circle cx="8" cy="8" r="2.4" />
                        </svg>
                    </span>
                    <span class="rail-txt">
                        <span class="rail-name">{{ stage.name }}</span>
                        <span class="rail-time">{{ stage.note }}</span>
                    </span>
                </div>

                <span
                    v-if="index < stages.length - 1"
                    :class="isBroken(index) ? 'seg-broken' : 'rail-seg'"
                    :title="isBroken(index) ? '流程在此中断' : undefined"
                />
            </div>
        </div>

        <div v-if="status === 'failed' && errorDetail" class="rail-error">
            <div class="rail-error-h">
                <span class="cap">停在 · {{ failedName ?? "未知阶段" }}</span>
                <span class="chip chip--bad">exit ≠ 0</span>
            </div>
            <pre>{{ errorDetail }}</pre>
            <div class="rail-error-act">
                <slot name="error-actions" />
            </div>
        </div>

        <div v-if="status === 'running'" class="rail-live">
            <span class="chip chip--run">进行中</span>
            <span class="rail-live-t">完整日志在任务结束后落库</span>
        </div>
    </div>
</template>
