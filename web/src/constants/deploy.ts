import type { DeployMode } from "../api/api";

/**
 * 状态词表的唯一定义处。
 *
 * 之前中文标签散落在各视图里各写一遍，同一个 status 在不同页面可能显示成
 * 不同措辞，新增状态时也总会漏掉一两处。这里集中一次。
 */
export type ChipVariant = "ok" | "run" | "bad" | "neu" | "tag";

/** 后端 deployments.status 的取值：queued / running / success / failed。 */
const STATUS_VARIANT: Record<string, ChipVariant> = {
  success: "ok",
  failed: "bad",
  running: "run",
  queued: "neu",
};

const STATUS_LABEL: Record<string, string> = {
  success: "成功",
  failed: "失败",
  running: "执行中",
  queued: "排队中",
  /** 「无更新」其实是一次成功的判断，不该显示成排队中。 */
  nochange: "无更新",
};

export function statusVariant(status: string): ChipVariant {
  return STATUS_VARIANT[status] ?? "neu";
}

export function statusLabel(status: string): string {
  return STATUS_LABEL[status] ?? status;
}

/**
 * 后端把「无更新」也记成 success，靠 message 区分：
 * `No update found: <项目名>`。部署成功则是 `Deployment succeeded: <项目名>`。
 */
export function isNoChange(message: string | null | undefined): boolean {
  return (message ?? "").startsWith("No update found");
}

/**
 * release 部署的版本号藏在 message 里：
 * `Triggered by release v1.2.3 (app.zip)`。
 * 后端没有单独的 version 字段，所以这里解析出来做展示。
 */
export function versionFromMessage(
  message: string | null | undefined,
): string | null {
  const match = /Triggered by release (\S+)/.exec(message ?? "");
  return match ? match[1] : null;
}

/** 展示用状态：无更新的成功单独成一档。 */
export function displayStatus(
  status: string,
  message: string | null | undefined,
): string {
  if (status === "success" && isNoChange(message)) {
    return "nochange";
  }
  return status;
}

/* ------------------------------------------------------------------
   发布阶段 —— 与后端执行顺序一一对应
   ------------------------------------------------------------------ */

/** local 模式：main.rs check_and_deploy_project。 */
export const LOCAL_STAGES = [
  "拉取仓库",
  "执行构建",
  "停止服务",
  "拷贝制品",
  "启动服务",
] as const;

/** ci_ssh 模式：deploy.rs deploy_remote_artifact。 */
export const CI_SSH_STAGES = [
  "下载制品",
  "建立 SSH",
  "执行停止命令",
  "上传制品",
  "解压到部署目录",
  "执行启动命令",
] as const;

export function stagesForMode(mode: DeployMode): readonly string[] {
  return mode === "ci_ssh" ? CI_SSH_STAGES : LOCAL_STAGES;
}

export type RailState = "is-done" | "is-now" | "is-bad" | "is-idle";

export interface RailStage {
  name: string;
  state: RailState;
  /** 该阶段的耗时或状态短语（"待执行" / "未启动"）。 */
  note: string;
}

export interface DeployRailInput {
  status: string;
  mode: DeployMode;
  /** 运行中：当前进行到第几个阶段（0 起）。 */
  nowStage?: number | null;
  /** 失败：卡在第几个阶段（0 起）。 */
  failedStage?: number | null;
  /** 已完成阶段的行号，用于从日志里取耗时。 */
  stageDurations?: (string | null)[];
  /** 排队且尚未开始——整条轨道都是「未启动」。 */
  halted?: boolean;
}

/**
 * 由部署记录推导轨道状态。
 *
 * 后端只在任务结束时落库日志，运行中的阶段无法从日志判断，因此
 * `nowStage` / `failedStage` 由调用方从日志行推导；推导不出来时轨道
 * 整体保持 idle，而不是假装有进度。
 */
export function buildRail(input: DeployRailInput): RailStage[] {
  const stages = stagesForMode(input.mode);
  const durations = input.stageDurations ?? [];

  return stages.map((name, index) => {
    const dur = durations[index] ?? null;

    if (input.halted) {
      return { name, state: "is-idle" as RailState, note: "未启动" };
    }

    if (input.status === "failed" && input.failedStage != null) {
      if (index < input.failedStage) {
        return { name, state: "is-done" as RailState, note: dur ?? "—" };
      }
      if (index === input.failedStage) {
        return { name, state: "is-bad" as RailState, note: "失败" };
      }
      return { name, state: "is-idle" as RailState, note: "未执行" };
    }

    if (input.status === "running" && input.nowStage != null) {
      if (index < input.nowStage) {
        return { name, state: "is-done" as RailState, note: dur ?? "—" };
      }
      if (index === input.nowStage) {
        return { name, state: "is-now" as RailState, note: "执行中" };
      }
      return { name, state: "is-idle" as RailState, note: "待执行" };
    }

    if (input.status === "success") {
      // 「无更新」只走完第一步就返回了，其余阶段标记为跳过
      if (dur) {
        return { name, state: "is-done" as RailState, note: dur };
      }
      return {
        name,
        state: "is-idle" as RailState,
        note: input.stageDurations ? "已跳过" : "—",
      };
    }

    return { name, state: "is-idle" as RailState, note: "待执行" };
  });
}

/* ------------------------------------------------------------------
   日志解析
   ------------------------------------------------------------------ */

const CMD_RE = /^\$ (ssh )?/;
const ERR_PREFIX_RE = /^(error|fatal|panic|unzip:|tar:|Command failed)/i;

export type LogLineKind = "cmd" | "out" | "err" | "ok" | "mark" | "plain";

export interface LogLine {
  no: number;
  kind: LogLineKind;
  text: string;
}

/**
 * 后端写入的日志有稳定格式：
 *   - 阶段标记：`Start ...` / `Remote deploy completed: ...`
 *   - 命令：以 `$ ` 或 `$ ssh ` 开头
 *   - 输出块：`stdout:` / `stderr:` 之后的多行
 *   - 错误：`Command failed: ...` 等
 *
 * 解析成带类型的行，前端据此着色并推导失败阶段。
 */
export function parseLog(text: string | null | undefined): LogLine[] {
  if (!text) {
    return [];
  }

  const lines: LogLine[] = [];
  let inErr = false;

  text.split(/\r?\n/).forEach((raw, index) => {
    const line = raw.replace(/\s+$/, "");
    let kind: LogLineKind = "plain";

    if (/^stderr:/.test(line)) {
      inErr = true;
      kind = "out";
    } else if (/^stdout:/.test(line)) {
      inErr = false;
      kind = "out";
    } else if (CMD_RE.test(line)) {
      inErr = false;
      kind = "cmd";
    } else if (line.startsWith("Start ") || line.startsWith("Connecting SSH")) {
      kind = "mark";
    } else if (/completed:/i.test(line)) {
      kind = "ok";
    } else if (inErr || ERR_PREFIX_RE.test(line)) {
      kind = "err";
    } else if (line === "") {
      kind = "plain";
    } else if (inErr) {
      kind = "err";
    } else {
      kind = "out";
    }

    lines.push({ no: index + 1, kind, text: line });
  });

  return lines;
}

/**
 * 从日志里推断失败停在哪个阶段：
 * 数一下失败前出现过多少条命令，命令数与阶段数大致对应。
 *
 * 这是启发式——后端没有记录阶段边界，所以只在能明确判断时返回，
 * 否则返回 null 让轨道保持 idle，不编造进度。
 */
export function failedStageFromLog(lines: LogLine[]): number | null {
  const firstErr = lines.findIndex((l) => l.kind === "err");
  if (firstErr < 0) {
    return null;
  }
  const commandsBefore = lines
    .slice(0, firstErr)
    .filter((l) => l.kind === "cmd").length;
  return commandsBefore > 0 ? commandsBefore : null;
}

/** 运行中的部署：最后一条命令对应正在执行的阶段。 */
export function runningStageFromLog(lines: LogLine[]): number | null {
  const commands = lines.filter((l) => l.kind === "cmd").length;
  return commands > 0 ? commands : null;
}
