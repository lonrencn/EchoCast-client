/**
 * 任务通知的传输模型，与 src-tauri/src/models.rs 的 TaskNotice 一一对应。
 *
 * 这一层只描述「服务端推下来的是什么」，不掺任何视图字段。
 * 组件要用的东西由 adapt.ts 转换 —— 传输模型和视图模型必须分开，
 * 否则后端加个字段就要改组件。
 */

export type TaskStatus = "running" | "success" | "warning" | "failed";

export type ExecutorKind = "local" | "remote" | "agent";

export interface Executor {
  name: string;
  /** 节点名，例如「GPU 节点 02」。单机场景可以不给。 */
  node?: string;
  kind?: ExecutorKind;
}

export type ActionStyle = "primary" | "ghost";

/** 气泡上的动作按钮。文案由服务端给，前端不硬编码「重试」这类词。 */
export interface NoticeAction {
  id: string;
  label: string;
  style?: ActionStyle;
  /** 点击后回传给后端的载荷。 */
  payload?: string;
}

/**
 * 问询：这条任务需要人给一个答复。
 *
 * 和 `actions` 的区别是语义，不是形状：`actions` 是「你可以做点什么」，
 * 点完就结束；`ask` 是「我在等你选一个」，在答复之前这条任务不算走完，
 * 所以要常驻、要出声（还会重复）、要在气泡上明说「等你答复…」。
 */
export interface NoticeAsk {
  /** 一句话问题。气泡放不下，只在展开的面板里出现。 */
  question: string;
  /** 2–4 个选项。少于 2 个不成立，多于 4 个客户端排版会挤掉问题本身。 */
  options: AskOption[];
  /** 答复截止时间（毫秒）。过期只是提示，仍然可答。 */
  deadline?: number;
  /** 原样回传给服务端的上下文，客户端不解释它。 */
  context?: string;
}

export interface AskOption {
  id: string;
  label: string;
  style?: ActionStyle;
  payload?: string;
}

/** 用户的选择。只存本地：服务端拿到的是 action 帧。 */
export interface AnswerRecord {
  optionId: string;
  label: string;
  at: number;
  /**
   * `ask` = 回答问题（选项退回来可以改选）；
   * `action` = 点了普通按钮（没有可重选的选项，只能说「再点一次」）。
   *
   * 两者共用这张表，是因为它们要显示的是同一件事：
   * 这一下到底有没有被服务端收下。
   */
  kind?: "ask" | "action";
  /**
   * 这次答复没有落地。两种情况共用这个标记，靠 `reason` 区分：
   * 没有 reason = 本机就没发出去（没连上）；有 reason = 服务端明确拒了。
   */
  failed?: boolean;
  /** 服务端给的原因（`already_answered` 这类）。有值说明是对面拒的，不是网络问题。 */
  reason?: string;
  /**
   * 发出去了，但服务端一直没回执。
   *
   * 既不能算成功也不能算失败 —— 有些服务端根本不回执（现在这台就是）。
   * 界面要如实说「答复已发出，对方没确认」，而不是给一个漂亮的绿色「已答复」。
   */
  unconfirmed?: boolean;
}

/** 媒体附件类型。服务端没给 kind 时由 Rust 侧按 mime/扩展名猜好再推下来。 */
export type MediaKind = "image" | "audio" | "video" | "file";

/**
 * 通知携带的多媒体附件。
 *
 * url 必须是 http(s) 可拉取的资源 —— base64 内嵌会被整条丢弃
 * （WS 文本帧塞几十兆的图，缓冲和解析两头都受不了）。
 * 下载由 Rust 侧 `media_fetch` 完成并缓存，前端只拿本地路径。
 */
export interface MediaItem {
  kind: MediaKind;
  url: string;
  /** 显示名。缺省时用 URL 最后一段。 */
  name?: string;
  mime?: string;
  /** 字节数，仅用于展示。 */
  size?: number;
  /** 视频封面图 URL。 */
  poster?: string;
}

export interface TaskNotice {
  id: string;
  /** 任务名，单行省略。是整个气泡唯一的扫视定位点。 */
  task: string;
  executor: Executor;
  status: TaskStatus;
  /** 结果摘要，两行封顶。只陈述事实，不复述「成功/失败」。 */
  result?: string;
  /** 展开后的完整内容或日志尾部。 */
  detail?: string;
  /** 0–1，仅 running 有意义。 */
  progress?: number;
  /** 最多两个，由后端保证，前端再截一次。 */
  actions?: NoticeAction[];
  /** 需要人答复的任务。有它时动作区让位给「等你答复…」。 */
  ask?: NoticeAsk;
  /** 覆盖默认常驻判定；不传则按 status 推断。 */
  sticky?: boolean;
  /** 毫秒级时间戳。 */
  timestamp: number;
  /** 指定形象包；不传则跟随全局设置。 */
  buddy_pack?: string;
  channel?: string;
  /** 这条任务被路由到哪台终端。客户端不参与路由，只用于展示与排障。 */
  terminal?: string;
  /** 离线补投时的原始创建时间（毫秒）。有值说明这条是上线后补的。 */
  offline_since?: number;
  /** 多媒体附件。有值时详情面板展示媒体区，气泡上出现「📎 N」角标。 */
  media?: MediaItem[];
  /** agent 属名（署名，`agent_name` / `node_alias` 等价）。展示在状态标题后的小字，超长由适配层掩码。 */
  agent_name?: string;
  /** 系统自产的提示气泡（欢迎/待命等）：展示但不出声。 */
  silent?: boolean;
}

/** 这条通知是不是在等人答复。选项不足两个的一律当普通通知处理。 */
export function isAsk(n: TaskNotice): boolean {
  return Boolean(n.ask && n.ask.options && n.ask.options.length >= 2);
}

/** 失败与警告必须常驻，用户没看见就等于没通知。 */
export function isSticky(n: TaskNotice): boolean {
  if (typeof n.sticky === "boolean") return n.sticky;
  // 问询必须常驻：没人答复它就永远不算结束，自己消失等于把这个球丢了
  if (isAsk(n)) return true;
  return n.status === "failed" || n.status === "warning";
}

/** 执行方渲染成「名字 · 节点」。 */
export function executorLabel(e: Executor): string {
  return e.node ? `${e.name} · ${e.node}` : e.name;
}

/** 相对时间。超过 24 小时转成绝对时间，否则「3 天前」没有信息量。 */
export function relativeTime(ts: number, now = Date.now()): string {
  const diff = Math.max(0, now - ts);
  const min = Math.floor(diff / 60_000);
  if (min < 1) return "刚刚";
  if (min < 60) return `${min} 分钟前`;
  const hour = Math.floor(min / 60);
  if (hour < 24) return `${hour} 小时前`;
  const d = new Date(ts);
  const pad = (v: number) => String(v).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
