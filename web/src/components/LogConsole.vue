<script setup lang="ts">
import { computed, ref } from "vue";
import { parseLog } from "../constants/deploy";

const props = defineProps<{
  logText: string | null;
}>();

const errorsOnly = ref(false);
const copied = ref(false);

const lines = computed(() => parseLog(props.logText));
const errorCount = computed(
  () => lines.value.filter((l) => l.kind === "err").length,
);

async function copy() {
  if (!props.logText) {
    return;
  }
  try {
    await navigator.clipboard.writeText(props.logText);
    copied.value = true;
    window.setTimeout(() => {
      copied.value = false;
    }, 1600);
  } catch {
    copied.value = false;
  }
}

function download() {
  if (!props.logText) {
    return;
  }
  const blob = new Blob([props.logText], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "deployment.log";
  link.click();
  URL.revokeObjectURL(url);
}
</script>

<template>
    <div class="term">
        <div class="term-h">
            <span class="cap">Execution log</span>
            <div class="term-act">
                <button
                    class="term-b"
                    :class="{ 'is-on': errorsOnly }"
                    type="button"
                    @click="errorsOnly = !errorsOnly"
                >
                    {{ errorsOnly ? "显示全部" : `仅看错误${errorCount ? ` (${errorCount})` : ""}` }}
                </button>
                <button class="term-b" type="button" @click="copy">
                    {{ copied ? "已复制" : "复制" }}
                </button>
                <button class="term-b" type="button" @click="download">下载</button>
            </div>
        </div>
        <div class="term-body" :class="{ 'is-errs': errorsOnly }">
            <div v-if="lines.length === 0" class="term-l">
                <span class="term-n">1</span>
                <span class="term-t">暂无日志</span>
            </div>
            <template v-else>
                <div
                    v-for="line in lines"
                    :key="line.no"
                    class="term-l"
                    :class="`is-${line.kind}`"
                >
                    <span class="term-n">{{ line.no }}</span>
                    <span class="term-t">{{ line.text || " " }}</span>
                </div>
            </template>
        </div>
    </div>
</template>
