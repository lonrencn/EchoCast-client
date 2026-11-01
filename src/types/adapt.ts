import {
  executorLabel,
  isAsk,
  relativeTime,
  type AnswerRecord,
  type MediaItem,
  type TaskNotice,
} from "../types/notice";

/** 气泡组件实际消费的视图模型。组件永远只认这个，不认传输模型。 */
export interface BubbleView {
  state: "success" | "running" | "fail" | "idle";
  /** 大标题。Glow 版用 */
  headline: string;
  /** 两字状态词。顶栏一行放不下大标题，用它代替 */
  badge: string;
  /** 任务名。Quiet 版用作 title */
  task: string;
  /** 执行方，已拼好「名字 · 节点」 */
  executor: string;
  /** 只到名字。灯条一条挤两条时用它，避免截成「Soundao · Ed…」这种断在分隔符上的半截 */
  executorShort: string;
  /** 执行节点。和名字分开给，窄的时候能整块丢掉 */
  node: string;
  result: string;
  /** 展开后才能看的完整内容（日志尾部 / 链接 / 长文本）。 */
  detail: string;
  time: string;
  progress?: number;
  accent?: string;
  actions: { id: string; label: string; primary?: boolean; payload?: string }[];
  /** 有值表示这条在等人答复；气泡的动作区让位给「等你答复…」。 */
  ask?: { question: string; options: { id: string; label: string; primary?: boolean }[] } | null;
  /** 已答复时不再显示「等你答复…」，改显示选了什么。 */
  answered?: AnswerRecord | null;
  /** 多媒体附件。气泡只显示数量角标，真正的展示在详情面板里。 */
  media?: MediaItem[];
  pack?: string;
}

const HEADLINE: Record<string, string> = {
  success: "任务完成",
  running: "正在执行",
  failed: "任务失败",
  warning: "需要留意",
};

/**
 * 顶栏用的两字状态词。
 *
 * 不复用 headline 的前两个字：`任务失败`.slice(0,2) 是「任务」，
 * 截字符串看起来省事，实际会得到一组毫无意义的结果。
 */
const BADGE: Record<string, string> = {
  success: "完成",
  running: "执行中",
  failed: "失败",
  warning: "留意",
};

/**
 * 传输模型 → 视图模型。
 *
 * 两个刻意的映射：
 * 1. warning 复用 fail 的形态，但用琥珀色覆盖 accent —— 组件只有四态，
 *    多加一态会让所有状态判断散开，用一个 accent 就能区分轻重。
 * 2. 动作最多两个，超出的截断。这是布局硬约束，后端保证了这里再兜一次。
 *
 * `answered` 是本地状态（用户点了哪个选项），不在传输模型里 ——
 * 服务端只从 action 帧知道这件事，没必要再回推一个「已回答」字段。
 */
export function toBubbleView(n: TaskNotice, answered?: AnswerRecord | null): BubbleView {
  const state: BubbleView["state"] =
    n.status === "running" ? "running" : n.status === "success" ? "success" : "fail";

  return {
    state,
    headline: HEADLINE[n.status] ?? "任务结束",
    badge: BADGE[n.status] ?? "结束",
    task: n.task,
    executor: executorLabel(n.executor),
    executorShort: n.executor.name,
    node: n.executor.node ?? "",
    result: n.result ?? "",
    detail: n.detail ?? "",
    time: relativeTime(n.timestamp),
    progress: n.status === "running" ? n.progress : undefined,
    accent: n.status === "warning" ? "#E0A030" : undefined,
    actions: (n.actions ?? []).slice(0, 2).map((a) => ({
      id: a.id,
      label: a.label,
      primary: a.style === "primary",
      payload: a.payload,
    })),
    ask: isAsk(n)
      ? {
          question: n.ask!.question,
          options: n.ask!.options.slice(0, 4).map((o) => ({
            id: o.id,
            label: o.label,
            primary: o.style === "primary",
          })),
        }
      : null,
    answered: answered ?? null,
    media: n.media ?? [],
    pack: n.buddy_pack,
  };
}
