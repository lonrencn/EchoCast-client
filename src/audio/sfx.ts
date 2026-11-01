/**
 * 提示音：一条通知换一个声音。
 *
 * 三条设计约束，都是被「通知频率」逼出来的：
 *
 * 1. **只在状态变化时响**，不跟着进度帧响。执行中的进度每几百毫秒一条，
 *    跟着响就变成打字机 —— 用户第一次遇到就会去把声音关掉，之后再也不开。
 * 2. **同一种音有最小间隔**。两条任务同时结束，响两声是合理的；
 *    十条同时结束还响十声就是故障现场。
 * 3. **失败和警告更长、更沉**，因为它们必须被看见；成功最短，因为它最多。
 * 4. **问询（ask）是唯一会重复的音**。它说的不是「事情怎么样」，而是
 *    「轮到你了」，所以用同音三连（dididi）而不是旋律；没人答复就隔一会儿再敲。
 *
 * 还有一条不是「通知频率」逼出来的，但同样硬：**静音是硬闸门**。
 * `sound_enabled` 关掉之后，连试听都不许出声 —— 只要有一处能绕过它，
 * 用户就会认定这个开关是假的，然后去任务管理器里关掉整个程序。
 *
 * 音频文件由 tools/make-sfx.py 生成，不要直接替换 wav —— 波形参数在脚本里，
 * 直接换文件会让「四种音是同一套音色」这件事散掉。
 */

import { watch } from "vue";
import startUrl from "../assets/sfx/start.wav";
import successUrl from "../assets/sfx/success.wav";
import warnUrl from "../assets/sfx/warn.wav";
import failUrl from "../assets/sfx/fail.wav";
import askUrl from "../assets/sfx/ask.wav";
import { settings } from "../settings/useSettings";
import type { TaskStatus } from "../types/notice";

export type SfxKey = "start" | "success" | "warn" | "fail" | "ask";

const URLS: Record<SfxKey, string> = {
  start: startUrl,
  success: successUrl,
  warn: warnUrl,
  fail: failUrl,
  ask: askUrl,
};

/** 同种音之间的最小间隔（毫秒）。 */
const MIN_GAP: Record<SfxKey, number> = {
  start: 1200,
  success: 900,
  warn: 2000,
  fail: 2000,
  // ask 会在没人答复时反复播，间隔由 feed 的追问节奏控制，
  // 这里只要挡住「三条问询同时到」叠成一声长鸣。
  ask: 2600,
};

/**
 * 每种音两路轮转。只有一路时，第二条通知会掐断第一条的尾音，
 * 听感上像「卡了一下」；两路足够应付真实场景，又不会叠成噪音。
 */
const POOL_SIZE = 2;

const pools = new Map<SfxKey, HTMLAudioElement[]>();
const cursor = new Map<SfxKey, number>();
const lastAt = new Map<SfxKey, number>();

/** 浏览器/WebView 尚未拿到用户交互，play() 会被拒。拒过一次就别再硬试。 */
let blocked = false;
/** 首次用户交互后置位，用于决定要不要重试被拒的那次。 */
let unlocked = false;

const stats = { plays: 0, skipped: 0, last: "" as SfxKey | "" };

function clamp01(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(1, Math.max(0, v));
}

/** 现在到底会不会出声。开关关掉、或音量拖到 0，都算静音。 */
export function isAudible(): boolean {
  return Boolean(settings.value.sound_enabled) && clamp01(settings.value.sound_volume ?? 0.7) > 0.001;
}

/**
 * 立刻把所有正在响的掐掉。
 *
 * 静音开关必须是**瞬时**的：用户点下去的那一刻耳朵里还有尾音，
 * 他会以为没生效。`pause()` 之后把 head 归零，下次播放才不会从半截开始。
 */
export function stopAllSfx(): number {
  let n = 0;
  for (const list of pools.values()) {
    for (const el of list) {
      if (!el.paused) {
        el.pause();
        n++;
      }
      try {
        el.currentTime = 0;
      } catch {
        // 元数据还没加载完时可能抛，忽略
      }
    }
  }
  return n;
}

/**
 * 谁把开关关掉都行（快捷方式、灯条上的喇叭、设置面板），但出口只有一个：
 * 在这里统一掐声音。放在设置变更处而不是按钮回调里，才能覆盖所有入口。
 */
watch(
  () => [settings.value.sound_enabled, settings.value.sound_volume] as const,
  () => {
    if (!isAudible()) stopAllSfx();
  }
);

function makeTrack(key: SfxKey): HTMLAudioElement {
  const el = new Audio(URLS[key]);
  el.preload = "auto";
  // 提示音不该跟着系统媒体键走，也不该出现在「正在播放」里
  el.setAttribute("aria-hidden", "true");
  return el;
}

function pool(key: SfxKey): HTMLAudioElement[] {
  let list = pools.get(key);
  if (!list) {
    list = Array.from({ length: POOL_SIZE }, () => makeTrack(key));
    pools.set(key, list);
  }
  return list;
}

function nextTrack(key: SfxKey): HTMLAudioElement {
  const list = pool(key);
  const i = (cursor.get(key) ?? 0) % list.length;
  cursor.set(key, i + 1);
  return list[i];
}

/** 首次交互后解锁。桌面包里靠 WebView2 的 autoplay 参数直接放行，这层是兜底。 */
function installUnlock() {
  if (typeof document === "undefined") return;
  const onGesture = () => {
    unlocked = true;
    blocked = false;
    document.removeEventListener("pointerdown", onGesture, true);
    document.removeEventListener("keydown", onGesture, true);
  };
  document.addEventListener("pointerdown", onGesture, true);
  document.addEventListener("keydown", onGesture, true);
}

installUnlock();

/** 状态 → 音。running 用 start：第一次出现才响，后续进度不响。 */
export function sfxForStatus(status: TaskStatus): SfxKey {
  switch (status) {
    case "success":
      return "success";
    case "warning":
      return "warn";
    case "failed":
      return "fail";
    default:
      return "start";
  }
}

/**
 * 播一个提示音。返回是否真的播了（被开关、音量或节流拦下都算没播）。
 * `force` 用于设置面板的试听：越过节流和「没有用户交互」的限制，
 * 但**越不过静音** —— 静音是用户明确表达的意图，任何代码路径都不该覆盖它。
 */
export function playSfx(key: SfxKey, opts: { force?: boolean } = {}): boolean {
  const force = opts.force === true;
  const volume = clamp01(settings.value.sound_volume ?? 0.7);

  // 硬闸门，先于一切：静音状态下连试听也不出声
  if (!settings.value.sound_enabled) {
    stats.skipped++;
    return false;
  }
  if (volume <= 0.001) {
    stats.skipped++;
    return false;
  }

  if (!force) {
    if (blocked && !unlocked) {
      stats.skipped++;
      return false;
    }
    const now = performance.now();
    if (now - (lastAt.get(key) ?? -Infinity) < (MIN_GAP[key] ?? 800)) {
      stats.skipped++;
      return false;
    }
    lastAt.set(key, now);
  }

  const el = nextTrack(key);
  el.volume = force ? Math.max(volume, 0.35) : volume;
  try {
    el.currentTime = 0;
  } catch {
    // 还没元数据时 currentTime 可能抛，忽略即可
  }

  stats.plays++;
  stats.last = key;

  const p = el.play();
  if (p && typeof p.catch === "function") {
    p.catch((err: DOMException) => {
      // NotAllowedError = 没有用户交互；标记后不再每次通知都去撞墙
      if (err?.name === "NotAllowedError") blocked = true;
    });
  }
  return true;
}

/** 调试/自动化用：看一眼现在到底播没播、被拦了多少次。 */
export function sfxDebug() {
  return { ...stats, blocked, unlocked, audible: isAudible() };
}

/** 试听用：按状态播，等价于收到一条该状态的通知。 */
export function previewSfx(status: TaskStatus): boolean {
  return playSfx(sfxForStatus(status), { force: true });
}
