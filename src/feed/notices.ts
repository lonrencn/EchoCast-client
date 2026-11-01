import { computed, reactive, ref } from "vue";
import {
  isAsk,
  isSticky,
  type AnswerRecord,
  type TaskNotice,
  type TaskStatus,
} from "../types/notice";
import { playSfx, sfxForStatus } from "../audio/sfx";
import { isConnected, sendAck, sendAction } from "../settings/useSettings";
import { isOpenableTarget, openExternal } from "../lib/open";
import { isSurface, windowKind } from "../boot/windowKind";

/**
 * 全应用共用一份通知列表。
 *
 * 气泡浮层、通知面板、托盘角标必须读同一个源 —— 三处各自维护一份状态，
 * 迟早会出现「面板里已经清掉了，角标还亮着」这种自相矛盾。
 */
const list = ref<TaskNotice[]>([]);

/** 同屏气泡上限。超出的折叠成「还有 N 条」。 */
export const MAX_VISIBLE = 3;

/** 保留上限。常驻的失败/警告不参与淘汰。 */
const MAX_STORED = 20;

/*
 * 通知生命周期 = 常驻 + 已读。
 *
 * 通知浮出后**不再定时消失**。渐隐只来自用户的三个动作：
 *   ① 悬停 10 秒（气泡上有倒计时条，移开鼠标即取消）→ 渐隐已读
 *   ② 点气泡上的「已读」→ 立即收起
 *   ③ 问询点了选项 / 动作按钮，走完流程
 * 看过详情面板再关上，也算已读。没读过的通知被新通知挤出可视区时
 * 折叠进「还有 N 条」，随时展开回看 —— 一条都不丢。
 */

/** 悬停多少毫秒算「已读」。 */
const HOVER_FADE_MS = 5_000;

/*
 * ── 消息回看（历史）────────────────────────────
 *
 * 收起的通告不等于消失：每条通知的最新快照（含答复状态）进历史，
 * 托盘「消息回看」能按任务名+时间翻出来，点一条就原样弹回桌面。
 *
 * 真相源在悬浮层（它负责显示与交互），历史写 localStorage ——
 * 多窗口同源共享，主窗口的回看列表直接读；主窗口只读不写，
 * 免得两边对同一条的「收起时刻」各说各话。
 */

export interface HistoryEntry {
  notice: TaskNotice;
  /** 答复快照。null = 这条还没人答（回放后仍可答复）。 */
  answered: AnswerRecord | null;
  /** 被收起的时刻；还在屏上的条目为 null。 */
  removedAt: number | null;
}

const HISTORY_KEY = "echocast.history";
const HISTORY_MAX = 100;

const historyMap = reactive(new Map<string, HistoryEntry>());

function loadHistory() {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (!raw) return;
    const arr = JSON.parse(raw) as HistoryEntry[];
    for (const e of arr) if (e?.notice?.id) historyMap.set(e.notice.id, e);
  } catch {
    /* 坏数据当没有，别让一条烂 JSON 弄哑整个回看 */
  }
}

function saveHistory() {
  try {
    const arr = [...historyMap.values()]
      .sort((a, b) => b.notice.timestamp - a.notice.timestamp)
      .slice(0, HISTORY_MAX);
    localStorage.setItem(HISTORY_KEY, JSON.stringify(arr));
  } catch {
    /* 存不下就算了，内存里还有这份 */
  }
}

loadHistory();

/** 主窗口打开回看页时调：悬浮层可能刚写过历史，重读一遍。 */
export function reloadHistory(): void {
  historyMap.clear();
  loadHistory();
}

/** 回看列表：时间倒序。 */
export const historyList = computed(() =>
  [...historyMap.values()].sort((a, b) => b.notice.timestamp - a.notice.timestamp)
);

/** 快照进历史。悬浮层专属（见上）；系统自产的欢迎气泡不算消息。 */
function snapshot(n: TaskNotice, removedAt: number | null) {
  if (n.silent || !isSurface) return;
  historyMap.set(n.id, {
    notice: { ...n },
    answered: answers.get(n.id) ?? null,
    removedAt,
  });
  saveHistory();
}

/** 清空回看记录。 */
export function clearHistory(): void {
  historyMap.clear();
  try {
    localStorage.removeItem(HISTORY_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * 把一条历史原样弹回桌面。
 *
 * 由主窗口回看列表发 `ec:notice-replay` 事件、悬浮层执行 —— 气泡、
 * 问询选项、链接、答复回执全部走既有管线，回看不需要第二套交互。
 * 关键一步是复位去重签名：这条通知的最后一帧八成和快照完全一致，
 * 不复位的话 upsert 会把它当重放帧整个吞掉，点了没反应。
 */
export function replayEntry(e: HistoryEntry): boolean {
  if (!e?.notice?.id) return false;
  historyMap.set(e.notice.id, e); // 本机没有这条历史时也能补齐
  saveHistory();
  lastSig.delete(e.notice.id);
  if (e.answered) answers.set(e.notice.id, e.answered);
  else answers.delete(e.notice.id);
  upsert({ ...e.notice });
  return true;
}

const timers = new Map<string, number>();

/**
 * 用户已经作出的答复。
 *
 * 放在 feed 而不是塞进 TaskNotice：答复是**本机的事实**，
 * 服务端只从 action 帧知道它，不需要再回推一个字段。
 * 混进传输模型，下次服务端推同一条时就会把「已答复」冲掉。
 */
const answers = reactive(new Map<string, AnswerRecord>());

/** 等答复时的追问节奏。第一次由状态变化触发，之后按这个间隔再敲。 */
const NAG_EVERY = 18_000;
/** 最多敲几次（含首响）。一直没人理就不敲了，吵到关程序比不提醒更糟。 */
const NAG_MAX = 3;

const nag = new Map<string, { count: number; timer: number }>();

/** 详情面板当前看的是哪一条。null = 关着。 */
export const sheetId = ref<string | null>(null);

/** 数据来源，用于在界面上明示「现在看到的是 mock」。 */
export const feedSource = ref<"pending" | "tauri" | "browser" | "none">("pending");

export const notices = computed(() => list.value);

/** 「还有 N 条」是否被点开。展开后全部滚出显示。 */
export const expanded = ref(false);
export function toggleExpanded() {
  expanded.value = !expanded.value;
}

/** 浮层里实际渲染的那几条。展开时全量。 */
export const visible = computed(() =>
  expanded.value ? list.value : list.value.slice(0, MAX_VISIBLE)
);

/** 被折叠的条数。展开时为 0。 */
export const overflow = computed(() =>
  expanded.value ? 0 : Math.max(0, list.value.length - MAX_VISIBLE)
);

/** 面板里正在看的那条。通知被清掉时自动变成 null，面板跟着收起。 */
export const sheetNotice = computed(
  () => list.value.find((n) => n.id === sheetId.value) ?? null
);

export function openSheet(id: string) {
  // 正在读的这条不能被悬停渐隐收走：人还在看，气泡一消失，面板跟着一起关。
  clearTimer(id);
  sheetId.value = id;
}

export function closeSheet() {
  // 看完详情 = 已读：关面板即收起，符合「常驻 + 已读」模型。
  const viewed = sheetId.value;
  sheetId.value = null;
  if (viewed && list.value.some((x) => x.id === viewed)) dismiss(viewed);
}

/**
 * 已读渐隐：鼠标滑过就算读过 —— 感应到后 5 秒自动收起。
 * 一旦感应就不撤销：移开鼠标不影响，人已经「碰到」过这条通知了。
 * 问询不调用这个（常驻等人处理，只能「忽略」或作答）。
 */
export function armReadFade(id: string) {
  if (timers.has(id)) return; // 已经在倒计时，别重置 —— 滑过一次就是一次
  timers.set(id, window.setTimeout(() => dismiss(id), HOVER_FADE_MS));
}

/**
 * 暂停某条的已读倒计时。
 *
 * 交付清单 / 详情开着的时候，人正在里面挑文件，气泡不能被判成
 * 「已读完」收走 —— 清单跟着气泡一起消失等于把交付物没收。
 * 移开鼠标再滑回来会重新计时（armReadFade 只看 timers 里有没有）。
 */
export function holdReadFade(id: string) {
  clearTimer(id);
}

/** 兼容保留：感应过的通知不因移开鼠标而取消倒计时。 */
export function cancelReadFade(_id: string) {
  /* no-op */
}

/** 某条通知的答复状态。组件拿它决定显示「等你答复…」还是「已答复 · X」。 */
export function answeredFor(id: string): AnswerRecord | null {
  return answers.get(id) ?? null;
}

function clearTimer(id: string) {
  const t = timers.get(id);
  if (t !== undefined) {
    clearTimeout(t);
    timers.delete(id);
  }
}

function trimStore() {
  if (list.value.length <= MAX_STORED) return;
  // 超限时只淘汰非常驻的，否则一条没人处理过的失败会被新通知挤掉
  const head = list.value.slice(0, MAX_STORED);
  const stickyTail = list.value.slice(MAX_STORED).filter(isSticky);
  list.value = [...head, ...stickyTail];
}

/**
 * 状态变了才响。
 *
 * 进度帧的 status 一直是 running，所以一轮任务只会响一次「接手」；
 * 只有落到终态时再响一次。离线补投的是历史事件，上线时可能一次来七八条，
 * 那几声叠在一起毫无信息量，直接不响。
 *
 * 问询优先于状态：一条「需要留意 + 等你答复」的任务，用户需要知道的
 * 不是「有个警告」，而是「轮到你说话了」，所以换成 ask 那串 dididi。
 */
function announce(before: TaskStatus | null, n: TaskNotice) {
  // 三个窗口都收得到 ec:notice，但声音只能有一个来源：
  // 谁真正把通知画在桌面上，谁负责响。主控制台只做静音预览。
  if (windowKind === "main") return;
  // 系统自产的提示气泡（欢迎/待命）不打扰 —— 出声是给「任务事件」的
  if (n.silent) return;
  if (before === n.status) return;
  if (n.offline_since) return;
  playSfx(isAsk(n) ? "ask" : sfxForStatus(n.status));
}

/** 停掉某条通知的追问。答复了、被收掉了、或者根本没在问，都要调它。 */
function stopNag(id: string) {
  const g = nag.get(id);
  if (g) {
    clearTimeout(g.timer);
    nag.delete(id);
  }
}

/**
 * 启动欢迎气泡：告诉用户「已经在待命了」。
 *
 * silent = 不出声 —— 开机自己叮一声是打扰，不是提醒。
 * 驻留比普通成功气泡长（8 秒）：它是告诉人「通知从哪冒出来」，不是报任务。
 */
function WELCOME_NOTICE(): TaskNotice {
  return {
    id: "sys-welcome",
    task: "EchoCast 已待命",
    executor: { name: "EchoCast", kind: "local" },
    status: "success",
    result: "通知通道就绪，有任务会第一时间告诉你",
    // 每次都取新时间戳：同一份内容切换形态后再弹一次，签名必须不同，
    // 否则 noticeSig 判定为重放帧、这次预览会被整个吞掉
    timestamp: Date.now(),
    silent: true,
  };
}

/** 在当前悬浮层的位置弹一条欢迎气泡。启动待命提示和「显示方式」预览共用。 */
export function showWelcome(): void {
  upsert(WELCOME_NOTICE());
}

/**
 * 没人答复就隔一会儿再敲一遍。
 *
 * 只在「这条还在屏幕上、还没答复、且不是离线补投」时继续。
 * 上限 NAG_MAX：提醒的目的是把人叫过来，不是把人逼到关掉程序。
 */
function startNag(n: TaskNotice) {
  stopNag(n.id);
  if (!isAsk(n) || answers.has(n.id) || n.offline_since) return;

  const tick = () => {
    const alive = list.value.some((x) => x.id === n.id);
    const g = nag.get(n.id);
    if (!alive || answers.has(n.id) || !g) {
      stopNag(n.id);
      return;
    }
    if (g.count >= NAG_MAX) {
      stopNag(n.id);
      return;
    }
    g.count += 1;
    playSfx("ask");
    g.timer = window.setTimeout(tick, NAG_EVERY);
  };

  nag.set(n.id, { count: 1, timer: window.setTimeout(tick, NAG_EVERY) });
}

/**
 * 同一帧的内容签名。服务端重连会重放队列，重放的帧和上次完全一致 ——
 * 这种帧不该重新调度显示、更不该出声。进度/状态/结果任何一个变了，
 * 签名就变，正常的进度更新不受影响。
 */
const lastSig = new Map<string, string>();

function noticeSig(n: TaskNotice): string {
  return [
    n.status,
    n.timestamp,
    n.progress ?? "",
    n.result,
    isAsk(n) ? JSON.stringify(n.ask) : "",
  ].join("|");
}

/** 同 id 覆盖而不是新增 —— 进度更新不能刷屏。 */
export function upsert(n: TaskNotice) {
  const i = list.value.findIndex((x) => x.id === n.id);

  // 完全相同的重放帧：什么都不要做。schedule 会让已收起的气泡重新弹出，
  // announce 会再响一声 —— 服务端反复重发时桌面上就像坏了一样。
  const sig = noticeSig(n);
  if (i >= 0 && lastSig.get(n.id) === sig) return;
  lastSig.set(n.id, sig);
  if (lastSig.size > 1024) lastSig.clear();

  // 同一条任务又来了一次问询（时间戳比上次答复更新）= 新一轮提问，
  // 旧的答案必须作废。不作废的话，服务端再问一次，气泡会直接显示
  // 「已答复 · 上一轮的选项」，用户再也没机会回答。
  const prevAnswer = answers.get(n.id);
  if (prevAnswer && isAsk(n) && n.timestamp > prevAnswer.at) answers.delete(n.id);
  const before = i >= 0 ? list.value[i].status : null;
  const wasAsk = i >= 0 ? isAsk(list.value[i]) : false;
  if (i >= 0) {
    list.value[i] = n;
  } else {
    list.value.unshift(n);
    trimStore();
  }
  announce(before, n);
  // 只在「刚刚变成问询」时起追问；同一轮的问询被重复推送不该重算次数
  if (isAsk(n) && (!wasAsk || i < 0)) startNag(n);
  if (!isAsk(n)) stopNag(n.id);
  snapshot(n, null); // 回看历史：永远留最新状态（含答复）
}

export function dismiss(id: string) {
  const gone = list.value.find((x) => x.id === id);
  if (gone) snapshot(gone, Date.now()); // 收起前留档，回看列表里还有它
  clearTimer(id);
  clearConfirm(id);
  stopNag(id);
  // 记录跟着气泡一起走。留着的话同一个 id 再次出现会直接显示
  // 「已答复 · 上一轮的选项」，用户再没机会作答。
  answers.delete(id);
  lastNackReason.delete(id);
  if (sheetId.value === id) sheetId.value = null;
  list.value = list.value.filter((x) => x.id !== id);
}

export function dismissAll() {
  for (const n of list.value) {
    snapshot(n, Date.now());
    clearTimer(n.id);
    clearConfirm(n.id);
    stopNag(n.id);
  }
  answers.clear();
  lastNackReason.clear();
  sheetId.value = null;
  list.value = [];
}

/**
 * 回答一条问询。
 *
 * 顺序刻意是「先记答案，再发帧」：面板里点下去必须立刻有反馈，
 * 而 WS 上行要等往返。发失败再把答案标成 failed —— 而不是先把界面
 * 恢复成「等你答复」，那会让用户以为自己没点中，然后再点一次。
 *
 * 答案以普通 action 帧上行：`{op:"action", notice_id, action_id:<选项 id>, payload}`。
 * 服务端拿 option id 就够判断了，不需要为问询单独开一种帧型。
 */
/** 等回复执的最长时间。超过就标「未确认」，但仍然保留答复记录。 */
const CONFIRM_WAIT = 4500;

const confirmTimers = new Map<string, number>();

export async function answerAsk(
  noticeId: string,
  optionId: string
): Promise<{ ok: boolean; reason?: string }> {
  const n = list.value.find((x) => x.id === noticeId);
  const opt = n?.ask?.options.find((o) => o.id === optionId);
  if (!n || !opt) return { ok: false, reason: "这条通知已经不在了" };

  answers.set(noticeId, { optionId, label: opt.label, at: Date.now() });
  stopNag(noticeId);

  const res = await sendAction(noticeId, opt.id, opt.payload ?? n.ask?.context);
  if (!res.ok) {
    answers.set(noticeId, {
      optionId,
      label: opt.label,
      at: Date.now(),
      failed: true,
    });
    return res;
  }

  // 答复后气泡**不再自动收起**：「已答复 · X」常驻显示，用户点「已读」
  // 或悬停 10 秒才收。服务端若还要继续，会推同 id 的新状态覆盖上来。
  //
  // 如果服务端回 nack，`applyActionResult` 会把答复记录改回失败态，
  // 气泡上重新出现可重选的提示 —— 常驻反而让这条线索更稳。

  // 等服务端表态。**这一步不是可选的礼貌**：现在的服务端完全不处理上行帧，
  // 没有它就等于给用户看一个绿色的「已答复」，而对面什么都没发生。
  // 收不到回执不判失败（慢和丢是两回事），但必须说实话。
  clearConfirm(noticeId);
  confirmTimers.set(
    noticeId,
    window.setTimeout(() => {
      const rec = answers.get(noticeId);
      if (!rec || rec.failed || rec.unconfirmed) return;
      answers.set(noticeId, { ...rec, unconfirmed: true });
    }, CONFIRM_WAIT)
  );

  return { ok: true };
}

function clearConfirm(id: string) {
  const t = confirmTimers.get(id);
  if (t !== undefined) {
    window.clearTimeout(t);
    confirmTimers.delete(id);
  }
}

/**
 * 服务端的 `reason` 是写给机器看的。`unknown_notice` 直接摊到界面上，
 * 用户只会以为程序坏了 —— 它其实在说「这条通知的 id 我这边没有记录」。
 */
const REASON_TEXT: Record<string, string> = {
  unknown_notice: "服务端不认识这条通知（id 对不上）",
  already_answered: "已被其他终端处理",
  expired: "这条已经过期",
  forbidden: "没有权限答这条",
  unauthorized: "没有权限答这条",
};

function humanReason(raw?: string): string {
  const r = raw?.trim();
  if (!r) return "服务端没有接受这次答复";
  return REASON_TEXT[r] ?? r;
}

/**
 * 服务端对上行动作的应答。
 *
 * 只有「这条是本机答的」才处理：别的终端答的、或者已经收起的，
 * 与本机无关。`ok` 为真什么都不用做 —— 气泡本来就会按 6 秒收起。
 * `ok` 为假则必须让用户看见，并且**撤掉收起定时器**。
 */
export function applyActionResult(o: {
  id: string;
  action_id?: string;
  ok: boolean;
  reason?: string;
}) {
  // 普通动作的回执：先放行等在那里的 `runAction`，再谈答复 ——
  // 二者不会同时存在（问询的气泡上根本没有普通动作按钮）。
  const exact = o.action_id ? ackKey(o.id, o.action_id) : null;
  const hit =
    (exact && actionWaiters.get(exact) ? exact : null) ??
    [...actionWaiters.keys()].find((k) => k.startsWith(`${o.id}\u0000`));
  if (hit) {
    if (!o.ok && o.reason) lastNackReason.set(o.id, o.reason);
    actionWaiters.get(hit)?.(o.ok);
    return;
  }

  const rec = answers.get(o.id);
  if (!rec) return;
  // 回的如果不是刚才那个选项，说明是上一轮的迟到回执，别去改现在的状态。
  if (o.action_id && o.action_id !== rec.optionId) return;

  // 有回执了，无论好坏，都不用再等。
  clearConfirm(o.id);

  if (o.ok) {
    if (rec.failed || rec.unconfirmed) {
      const { failed: _f, reason: _r, unconfirmed: _u, ...rest } = rec;
      answers.set(o.id, rest);
    }
    return;
  }

  clearTimer(o.id);
  answers.set(o.id, {
    ...rec,
    failed: true,
    reason: humanReason(o.reason),
  });
  // 原文留一份在控制台：用户看到的是人话，排障看的是原话。
  if (o.reason) console.warn("[feed] 答复被服务端拒绝:", o.reason);
}

/**
 * 动作发出后等回执的阀门。
 *
 * 为什么需要它：`conn_send_action` 成功只代表这一帧进了本地发送队列。
 * 服务端可能压根不认识这个 notice_id —— 此时它会回 nack，而气泡如果
 * 已经收掉了，用户永远不知道。「点了、气泡没了、什么都没发生」是最难
 * 排查的一种失败，因为界面上没有任何痕迹。
 */
const actionWaiters = new Map<string, (ok: boolean) => void>();
/** nack 的原因要先存一下：resolve 只传布尔，原因得另外交出去。 */
const lastNackReason = new Map<string, string>();

const ACTION_ACK_WAIT = 1500;

/**
 * 「查看详情」类动作的识别。
 *
 * 服务端契约里动作分两种：带 URL payload 的（点开浏览器）和不带的。
 * 不带 URL、标签又是「查看/详情/结果」这一族、且通知里确实有内容可看时，
 * 这个动作的目的是把 detail 摆到用户面前 —— 应该弹详情面板，
 * 而不是像接单类动作那样把气泡收掉。收掉气泡等于把要看的全文一起收走，
 * 用户看到的就是「一点查看详情就没了」。
 */
const VIEW_ACTION_RE = /查看|详情|结果|打开|detail|view|open/i;

function ackKey(id: string, actionId: string) {
  return `${id}\u0000${actionId}`;
}

function waitActionAck(
  id: string,
  actionId: string
): Promise<"ack" | "nack" | "timeout"> {
  return new Promise((resolve) => {
    const key = ackKey(id, actionId);
    const timer = window.setTimeout(() => {
      actionWaiters.delete(key);
      resolve("timeout");
    }, ACTION_ACK_WAIT);
    actionWaiters.set(key, (ok) => {
      window.clearTimeout(timer);
      actionWaiters.delete(key);
      resolve(ok ? "ack" : "nack");
    });
  });
}

/**
 * 点击气泡上的动作按钮。
 *
 * 回传走 WS 上行帧（`{op:"action", notice_id, action_id, payload}`）。
 * 连不上时不要假装成功：气泡留在原地让用户能再点一次，
 * 比「点了一下什么都没发生」清楚。
 *
 * 如果这个动作本身就是「接单 / 跳过」，再补一帧 `ack`。两条链路发的
 * 是同一个事实的不同侧面：`action` 是「点了什么」，`ack` 是「任务到哪一步了」。
 * 服务端做投递看板时只需要后者，所以不能只发前者。
 */
export async function runAction(noticeId: string, actionId: string) {
  const notice = list.value.find((n) => n.id === noticeId);
  const action = notice?.actions?.find((a) => a.id === actionId);
  const payload = action?.payload;
  // 先真的把内容打开，再回传。
  // 服务端契约里 payload 就是一个 URL（`{label:"查看结果", url:"…"}`）——
  // 之前只发 action 不打开，用户看到的是「点了没反应，气泡还消失了」。
  let opened = false;
  if (payload && isOpenableTarget(payload)) {
    opened = await openExternal(payload);
  }

  // 没有 URL 可开、标签是「查看」族、通知里又有内容可看 → 这是「查看详情」，
  // 弹面板出全文，气泡留住。接单/重试/忽略这类动作不命中，维持原收起行为。
  const viewLike =
    !opened &&
    Boolean((notice?.detail ?? notice?.result ?? "").trim()) &&
    (VIEW_ACTION_RE.test(action?.label ?? "") || VIEW_ACTION_RE.test(actionId));

  const res = await sendAction(noticeId, actionId, payload);

  // 接单类动作补一条任务级回执。首字母大写只是为了让 `Accept` 这类写法也能命中。
  const ackState =
    actionId.toLowerCase() === "accept"
      ? "accepted"
      : actionId.toLowerCase() === "reject"
        ? "rejected"
        : null;
  if (res.ok && ackState) {
    await sendAck(noticeId, ackState);
  }

  if (res.ok) {
    // 帧写进发送队列 ≠ 服务端收到了。ack 通常十几毫秒就回来，等这一下
    // 对体验几乎无感；而「先收气泡再看结果」，等于替服务端宣布了一次
    // 它没做过的事 —— 服务端回 nack 时用户看到的却是气泡干干净净消失。
    const verdict = await waitActionAck(noticeId, actionId);

    if (verdict === "nack") {
      // 不收气泡，把拒的原因写进去。普通动作没有「选项」可退，
      // 所以文案是「再点一次」而不是「重选」。
      answers.set(noticeId, {
        optionId: actionId,
        label: action?.label ?? actionId,
        at: Date.now(),
        kind: "action",
        failed: true,
        reason: humanReason(lastNackReason.get(noticeId)),
      });
      lastNackReason.delete(noticeId);
      return { ok: false, opened, reason: "服务端没有接受这个动作" };
    }

    // 查看类动作：面板就是这次点击的反馈，气泡留着（面板开着时
    // openSheet 已把驻留定时器撤了，不会读到一半被收走）。
    if (viewLike) {
      openSheet(noticeId);
      return { ok: true, opened };
    }

    dismiss(noticeId);
    return { ok: true, opened };
  }

  // 没连服务端（mock / 演示）时也走同一套反馈，否则按钮点了没反应更像 bug
  if (!isConnected.value) {
    if (viewLike) {
      openSheet(noticeId);
    } else {
      dismiss(noticeId);
    }
    return { ok: true, opened };
  }

  console.warn("[feed] 动作回传失败，气泡保留以便重试", res.reason);
  return { ok: false, opened, reason: res.reason };
}

let booting: Promise<void> | null = null;

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 建立数据源。桌面里订阅 Rust 推来的 ec:notice；
 * 浏览器里没有 IPC，改用前端自带 mock，保证 `pnpm dev` 也能看到效果。
 */
export function startFeed(): Promise<void> {
  if (booting) return booting;

  booting = (async () => {
    if (inTauri()) {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        await listen<TaskNotice>("ec:notice", (e) => upsert(e.payload));
        // 抢单场景：这条任务先被别的终端接了，本机要立刻收掉气泡。
        // 不收的话用户会对着一个已经被处理的任务点「开始」。
        await listen<{ id: string; reason?: string }>("ec:revoke", (e) => {
          if (e.payload?.id) dismiss(e.payload.id);
        });
        // 服务端对答复的应答。收不到就当「没回执」继续走，不当失败。
        await listen<{
          id: string;
          action_id?: string;
          ok: boolean;
          reason?: string;
        }>("ec:action-result", (e) => {
          if (e.payload?.id) applyActionResult(e.payload);
        });
        feedSource.value = "tauri";

        // 欢迎气泡：悬浮层启动后报一声「已待命」。不响铃、几秒自走，
        // 只在手上还没有任何通知时出 —— 有任务在跑就不用客气了。
        // 气泡层和顶部灯条都算通知面，两种形态都要报。
        if (isSurface) {
          window.setTimeout(() => {
            if (!list.value.length) showWelcome();
          }, 1200);
        }
        // 「显示方式」切换：主窗口广播 ec:welcome，让新形态立刻在自己的
        // 位置弹一条 —— 用户刚切完就该看到「通知以后从哪冒出来」，
        // 而不是等到第一条真任务才知道灯条长什么样。
        await listen("ec:welcome", () => {
          if (isSurface) showWelcome();
        });
        // 消息回看：主窗口的回看列表点了某条，历史快照整体发过来，
        // 悬浮层负责把它原样弹回桌面（答复/链接交互全走既有管线）。
        await listen<HistoryEntry>("ec:notice-replay", (e) => {
          if (isSurface && e.payload) replayEntry(e.payload);
        });
        return;
      } catch (err) {
        // 桌面端订阅失败也**绝不**回落浏览器 mock：
        // 演示数据混进真实通知里，用户分不清哪条是真的，比没数据更糟。
        console.warn("[feed] 订阅 ec:notice 失败，桌面端不使用演示数据", err);
        feedSource.value = "none";
        return;
      }
    }

    try {
      const { startBrowserMock } = await import("./browserMock");
      startBrowserMock(upsert);
      feedSource.value = "browser";
    } catch (err) {
      console.error("[feed] 前端 mock 启动失败", err);
      feedSource.value = "none";
    }
  })();

  return booting;
}

/** 手动推一条，不用等定时器。 */
export async function pushOnce() {
  if (inTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("mock_push", { round: Math.floor(Date.now() / 1000) % 4 });
    return;
  }
  const { pushBrowserOnce } = await import("./browserMock");
  pushBrowserOnce(upsert);
}

/**
 * 手动推一条**问询**。
 *
 * 固定轮次 1：mock 里唯一带 ask 的场景。演示要的是「按一下就出现」，
 * 跟着时间轮换会让人按三次才看到，等于没有这个按钮。
 */
export async function pushAskDemo() {
  if (inTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("mock_push", { round: 1 });
    return;
  }
  const { pushBrowserAsk } = await import("./browserMock");
  pushBrowserAsk(upsert);
}
