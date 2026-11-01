<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useBuddy } from "../buddies/useBuddy";
import type { BuddyAction } from "../buddies/types";
import type { AnswerRecord, MediaItem } from "../types/notice";
import LinkText from "./LinkText.vue";
import { linkify } from "../lib/linkify";
import {
  copyText,
  isOpenableUrl,
  looksLikePath,
  openExternal,
} from "../lib/open";
import { settings } from "../settings/useSettings";

type GlowState = "success" | "running" | "fail" | "idle";

interface GlowAction {
  id: string;
  label: string;
  primary?: boolean;
  /** 动作指向的目标：URL 或本机路径。卡片 / robot 点击的兜底来源。 */
  payload?: string;
}

interface GlowAsk {
  question: string;
  options: { id: string; label: string; primary?: boolean }[];
}

/** 动作交给上层处理：组件不知道「重试」到底该 invoke 什么。
 *  answer：问询选项直接在气泡上答（不再弹面板）。
 *  dismiss：点击卡片 / 「已读」/「忽略」= 收起这条。
 *  read-fade：鼠标滑过感应，定时器在 feed 层统一管（滑过即已读，5 秒后收走）。 */
const emit = defineEmits<{
  action: [id: string];
  answer: [optionId: string];
  dismiss: [];
  "read-fade": [on: boolean];
  /** 暂停已读倒计时：交付清单开着时人在挑文件，气泡不能被收走 */
  "hold-fade": [];
  /** 打开完整详情面板（媒体走 MediaGallery 预览） */
  "open-sheet": [];
}>();

const props = withDefaults(
  defineProps<{
    state?: GlowState;
    /** 执行方，已拼好「名字 · 节点」，显示在卡片顶部 */
    executor: string;
    /** 任务名 */
    task: string;
    /** 结果摘要 */
    result?: string;
    /** 完整内容。有它在，卡片就可以点开看全文 */
    detail?: string;
    time?: string;
    actions?: GlowAction[];
    /** 有值表示这条在等人答复 —— 动作区让位给「等你答复…」 */
    ask?: GlowAsk | null;
    /** 已经答复过：显示选了什么，而不是继续催 */
    answered?: AnswerRecord | null;
    progress?: number;
    /** 覆盖状态色。留空则按 state 取默认 */
    accent?: string;
    /** 覆盖大标题。留空则按 state 取「任务完成 / 执行中 / 任务失败」 */
    headline?: string;
    /** 多媒体附件。气泡上只显示「📎 N」角标，真正的展示在详情面板。 */
    media?: MediaItem[];
    /** 指定素材包 id。留空则跟随全局设置 —— 这是「图可换」的入口 */
    pack?: string;
  }>(),
  {
    state: "success",
    result: "",
    detail: "",
    time: "刚刚",
    actions: () => [],
    ask: null,
    answered: null,
  }
);

const ACCENT: Record<GlowState, string> = {
  success: "#2FD9A4",
  running: "#4DA6FF",
  fail: "#FF4D5E",
  idle: "#8B93A7",
};

const buddy = useBuddy(() => props.pack);

const HEADLINE: Record<GlowState, string> = {
  success: "任务完成",
  running: "正在执行",
  fail: "任务失败",
  idle: "待命中",
};

const hovering = ref(false);

/** 滑过即已读：感应到鼠标后 5 秒收走，移开鼠标也不撤销（人已经看过它了）。 */
const fading = ref(false);

function onEnter() {
  hovering.value = true;
  // 问询常驻等人处理；交付清单开着时人在里面挑，都不进入已读倒计时
  if (props.ask || deliveryOpen.value) return;
  fading.value = true;
  emit("read-fade", true);
}

function onLeave() {
  hovering.value = false;
}

const accentColor = computed(() => props.accent ?? ACCENT[props.state]);
const headlineText = computed(() => props.headline ?? HEADLINE[props.state]);

const settled = computed(() => props.state === "success" || props.state === "fail");
/// 只有包里真的有 running 帧才播呼吸，否则静止比错帧好
const breathing = computed(() => props.state === "running" && buddy.supports("running"));

/* ── 交付清单 ──────────────────────────────────────────
 *
 * 服务端「任务完成」的交付物有三个藏身处：正文里的裸链接 /
 * [文案](url)、动作按钮的 payload、media[] 附件。之前只看前
 * 两个 —— 服务端把文件链接走 media[] 下发时，气泡上什么都点
 * 不开，悬停提示永远是「点击收起」。
 *
 * 这里统一收进一份清单：robot 点击弹出清单逐项打开，卡片点击
 * 直接开第一项。同一去重（按 URL），链接在前、附件在后。
 */
interface Deliverable {
  icon: string;
  label: string;
  sub: string;
  url: string;
  /** 有值 = media 附件：优先打开已预取的本地缓存 */
  media?: MediaItem;
}

const MEDIA_ICONS: Record<string, string> = {
  image: "🖼",
  audio: "🎧",
  video: "🎬",
  file: "📄",
};
const MEDIA_LABELS: Record<string, string> = {
  image: "图片",
  audio: "音频",
  video: "视频",
  file: "文件",
};

function hostOf(u: string): string {
  try {
    return new URL(u).host;
  } catch {
    return u;
  }
}

function lastNameOf(u: string): string {
  try {
    const seg = new URL(u).pathname.split("/").filter(Boolean).pop();
    return seg ? decodeURIComponent(seg) : u;
  } catch {
    return u;
  }
}

const deliverables = computed<Deliverable[]>(() => {
  const out: Deliverable[] = [];
  const seen = new Set<string>();
  const pushLink = (url: string, label?: string) => {
    if (!isOpenableUrl(url) || seen.has(url)) return;
    seen.add(url);
    out.push({ icon: "🔗", label: label || url, sub: hostOf(url), url });
  };
  for (const t of [props.result, props.detail]) {
    if (!t) continue;
    for (const seg of linkify(t)) {
      if (seg.kind === "link") pushLink(seg.value, seg.label);
    }
  }
  for (const a of props.actions) {
    if (a.payload) pushLink(a.payload, a.label);
  }
  for (const m of props.media ?? []) {
    if (!isOpenableUrl(m.url) || seen.has(m.url)) continue;
    seen.add(m.url);
    out.push({
      icon: MEDIA_ICONS[m.kind] ?? "📄",
      label: m.name || lastNameOf(m.url),
      sub: `${MEDIA_LABELS[m.kind] ?? "文件"} · 点击用系统程序打开`,
      url: m.url,
      media: m,
    });
  }
  return out;
});

/** 第一项交付物。卡片点击直接开它，悬停提示也报它。 */
const firstTarget = computed(() => deliverables.value[0]?.url ?? "");

const currentSrc = computed(() =>
  deliverables.value.length
    ? buddy.src("excited" as BuddyAction) // 有交付 = 有好东西，robot 兴奋欢呼
    : buddy.src(props.state as BuddyAction)
);
/** 兴奋帧自带弹跳；没有 excited 帧的包退回 success/idle，就不加动画。 */
const cheering = computed(() => deliverables.value.length > 0 && buddy.supports("excited"));
const progressPct = computed(() =>
  props.progress === undefined ? null : Math.round(props.progress * 100)
);

/**
 * 通知里第一个可点开的目标。见 deliverables 注释 —— 正文链接、
 * 动作 payload、media 附件三处统一收口，这里只取第一项。
 */
const firstLink = computed<string>(() => firstTarget.value);

/** 点击的一行反馈。打不开时绝不静默 —— 至少把链接复制走。 */
const flash = ref("");
let flashTimer = 0;
function showFlash(msg: string) {
  flash.value = msg;
  window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => (flash.value = ""), 2400);
}

/** robot 受击动画：点一下弹一下，点了没反应比点错更伤信任。 */
const poked = ref(false);
function poke() {
  poked.value = false;
  requestAnimationFrame(() => (poked.value = true));
  window.setTimeout(() => (poked.value = false), 500);
}

/* ── 交付清单弹层 ──────────────────────────────────────
 * robot 点击 = 弹出全部交付物逐项打开。文件类（media）优先用
 * Rust 已预取的本地缓存交给系统默认程序；没有 IPC（浏览器预览）
 * 或缓存失败时退回直接开 URL。
 */
const deliveryOpen = ref(false);

function onDocClickClose() {
  deliveryOpen.value = false;
}

function onEsc(e: KeyboardEvent) {
  if (e.key === "Escape" && deliveryOpen.value) {
    deliveryOpen.value = false;
    e.stopPropagation();
  }
}

onMounted(() => {
  window.addEventListener("keydown", onEsc, true);
  document.addEventListener("click", onDocClickClose);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onEsc, true);
  document.removeEventListener("click", onDocClickClose);
});

/** robot 点击：受击 + 弹交付清单（没有交付物时退回收起）。 */
function onBuddyClick() {
  poke();
  if (!deliverables.value.length) {
    emit("dismiss");
    return;
  }
  deliveryOpen.value = !deliveryOpen.value;
  if (deliveryOpen.value) emit("hold-fade"); // 挑文件期间不判已读
}

/** media 附件：先取缓存路径，再用系统默认程序打开。 */
async function openMediaFile(m: MediaItem): Promise<boolean> {
  const inTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  if (inTauri) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const res = await invoke<{ path: string }>("media_fetch", {
        url: m.url,
        token: settings.value.token || null,
      });
      await invoke("media_open", { path: res.path });
      return true;
    } catch (e) {
      console.warn("[delivery] 缓存打开失败，退回直接开 URL", m.url, e);
    }
  }
  return openExternal(m.url);
}

/** 打开一项交付物。打不开时绝不静默 —— 至少把链接复制走。 */
async function openDeliverable(d: Deliverable) {
  const ok = d.media ? await openMediaFile(d.media) : await openExternal(d.url);
  if (ok) {
    showFlash(d.media || looksLikePath(d.url) ? "已交给系统打开" : "已在浏览器打开");
  } else {
    await copyText(d.url);
    showFlash("打不开，已复制链接");
  }
}

/** 点卡片：清单开着就先收清单；有交付物开第一项；否则已读收起。 */
async function onCardClick() {
  if (deliveryOpen.value) {
    deliveryOpen.value = false;
    return;
  }
  poke();
  const first = deliverables.value[0];
  if (!first) {
    emit("dismiss");
    return;
  }
  await openDeliverable(first);
}

/** 媒体附件角标。不带媒体时不渲染。 */
const mediaCount = computed(() => props.media?.length ?? 0);

/**
 * 动作被拒时的一行说明。
 *
 * 和「已答复」分开：答复失败要把选项退回来给用户重选，动作失败只能把
 * 按钮留在原位让他再点一次 —— 两者长得像，动作完全不同，混用一个分支
 * 会出现「说重选，却没有选项」。
 */
const actionFailed = computed(() => {
  const a = props.answered;
  if (!a || !a.failed || a.kind !== "action") return "";
  return a.reason ? "服务端没接受 · 再点一次" : "没发出去 · 再点一次";
});
</script>

<template>
  <div
    class="glow"
    :class="[`is-${state}`, { settled, hovering }]"
    :style="{ '--accent': accentColor }"
    @mouseenter="onEnter"
    @mouseleave="onLeave"
  >
    <!-- 角色：压在卡片左缘，半身在卡片之外。点它 = 弹交付清单
         （没有交付物时收起），受击时弹一下给出触感反馈。 -->
    <div
      class="buddy"
      :title="deliverables.length ? '点我查看交付' : '点我收起'"
      @click.stop="onBuddyClick"
    >
      <span class="aura" aria-hidden="true" />
      <img
        :src="currentSrc"
        class="buddy-img"
        :class="{ breathing, cheering, poked }"
        alt=""
        draggable="false"
      />
      <span class="ring" aria-hidden="true" />
    </div>

    <div
      class="card clickable"
      :title="firstLink ? `点击打开 ${firstLink}` : '点击收起'"
      @click="onCardClick"
    >
      <span class="sheen" aria-hidden="true" />

      <!-- 交付清单弹层：robot 点击弹出，逐项打开。放在卡片内部，
           位置贴底铺开，盖住正文但不超出气泡。 -->
      <Transition name="dpop">
        <div v-if="deliveryOpen && deliverables.length" class="delivery" @click.stop>
          <div class="d-head">
            <span>交付清单 · {{ deliverables.length }} 项</span>
            <button class="d-close" title="收起清单" @click.stop="deliveryOpen = false">
              ✕
            </button>
          </div>
          <div class="d-list">
            <button
              v-for="(d, i) in deliverables"
              :key="d.url + i"
              class="d-row"
              @click.stop="openDeliverable(d)"
            >
              <span class="d-ic">{{ d.icon }}</span>
              <span class="d-main">
                <span class="d-name">{{ d.label }}</span>
                <span class="d-sub">{{ d.sub }}</span>
              </span>
              <span class="d-go" aria-hidden="true">↗</span>
            </button>
          </div>
        </div>
      </Transition>

      <!-- 已读交互：右上角小按钮，悬停时显现（让位给时间戳） -->
      <button class="readbtn" title="标记已读并收起" @click.stop="emit('dismiss')">已读</button>

      <!-- 悬停倒计时条：10 秒从满到空，走完即已读收起 -->
      <span class="fadebar" :class="{ on: fading }" aria-hidden="true"><i /></span>

      <!-- 点击反馈：开了 / 收了 / 复制了，一句话说清这次点击干了什么 -->
      <Transition name="flashpop">
        <div v-if="flash" class="flashline" role="status">{{ flash }}</div>
      </Transition>

      <div class="topline">
        <span class="badge">
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true">
            <path
              d="M12 2.6 4.2 7v10L12 21.4 19.8 17V7L12 2.6Zm0 5.1a2.4 2.4 0 1 1 0 4.8 2.4 2.4 0 0 1 0-4.8Z"
              fill="currentColor"
            />
          </svg>
        </span>
        <span class="agent">{{ executor }}</span>
        <span class="time">{{ time }}</span>
      </div>

      <h3 class="headline">{{ headlineText }}</h3>

      <div class="meta">
        <div class="row">
          <svg class="ic" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M3 2.2 9.4 6 3 9.8V2.2Z" fill="currentColor" />
          </svg>
          <span class="k">任务</span>
          <span class="v">{{ task }}</span>
        </div>

        <div v-if="result" class="row">
          <svg class="ic" viewBox="0 0 12 12" aria-hidden="true">
            <path
              d="M2 6.4 4.6 9 10 3"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <span class="k">结果</span>
          <span class="v"><LinkText :text="result" /></span>
        </div>

        <div v-if="mediaCount > 0" class="row openable" title="点开完整预览（图片 / 播放 / 文件）" @click.stop="emit('open-sheet')">
          <svg class="ic" viewBox="0 0 12 12" aria-hidden="true">
            <path
              d="M4.2 8.8 8 5M5.4 9.9l1.1-1.1M7.6 3.3 8.7 2.2M2.1 9.9l3.2-3.2M6.5 5.5 9.9 2.1"
              fill="none"
              stroke="currentColor"
              stroke-width="1.2"
              stroke-linecap="round"
            />
          </svg>
          <span class="k">附件</span>
          <span class="v">📎 {{ mediaCount }} 个文件 · 点击预览</span>
        </div>
      </div>

      <div v-if="progressPct !== null" class="bar">
        <span class="bar-fill" :style="{ width: `${progressPct}%` }" />
        <span class="bar-pct">{{ progressPct }}%</span>
      </div>

      <!-- 动作被服务端拒了：说清楚，按钮留在下面让它能再点一次。
           「回答问题」才有选项可退给用户，「点按钮」没有，所以这条单独一行。 -->
      <div v-if="actionFailed" class="answered failed">
        <span class="tick">!</span>
        <span>{{ actionFailed }}</span>
      </div>

      <!-- 全文直接铺在气泡里：点卡片就是收起，没有第二层交互。
           文本里的链接可点（LinkText），点链接不会把气泡点没。 -->
      <div v-if="detail" class="detail-x">
        <LinkText :text="detail" />
      </div>

      <!-- ── 问询：选项直接摆在气泡上，答完即走 ─────────────
           不再「点一下进面板再选」——右下角这一张卡就是全部交互。 -->
      <div v-if="ask && !answered" class="ask-row">
        <span class="ask-q">
          <span class="didi" aria-hidden="true"><i /><i /><i /></span>
          {{ ask.question }}
        </span>
        <div class="ask-opts">
          <button
            v-for="o in ask.options"
            :key="o.id"
            class="ask-opt"
            :class="{ primary: o.primary }"
            @click.stop="emit('answer', o.id)"
          >
            {{ o.label }}
          </button>
          <button class="ask-ignore" title="不答复，先收起来" @click.stop="emit('dismiss')">
            忽略
          </button>
        </div>
      </div>

      <!-- 已答复：把「你选了什么」留在原地，比直接消失更让人放心。
           没答上时不写原因（气泡太窄），但必须说清「要不要再点一次」——
           只说失败、不说能重试，用户就只会关掉窗口。 -->
      <div
        v-else-if="answered && !actionFailed"
        class="answered"
        :class="{ failed: answered.failed, wait: answered.unconfirmed }"
      >
        <span class="tick">{{ answered.failed ? "!" : answered.unconfirmed ? "?" : "✓" }}</span>
        <span v-if="answered.failed">
          {{ answered.reason ? "服务端没接受" : "没发出去" }} · 点这里重选
        </span>
        <!-- 气泡太窄，说不清「已发出但对方没确认」，只点到为止 -->
        <span v-else-if="answered.unconfirmed">已发出 · 待确认 · {{ answered.label }}</span>
        <span v-else>已答复 · {{ answered.label }}</span>
      </div>

      <div v-else-if="actions.length" class="acts">
        <button class="cta" @click.stop="emit('action', actions[0].id)">
          {{ actions[0].label }}
          <svg viewBox="0 0 14 14" width="12" height="12" aria-hidden="true">
            <path
              d="M2.5 7h8M7.4 3.6 10.8 7l-3.4 3.4"
              fill="none"
              stroke="currentColor"
              stroke-width="1.7"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </button>
        <!-- 次要动作不能丢：数据模型给了两个，就必须都能点 -->
        <button
          v-if="actions[1]"
          class="cta-ghost"
          @click.stop="emit('action', actions[1].id)"
        >
          {{ actions[1].label }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 深色承托是这套风格的前提：辉光是靠"吃掉"暗背景来建立体积的 */
.glow {
  position: relative;
  width: 400px;
  max-width: 100%;
  padding-left: 104px;
  box-sizing: border-box;
  display: flex;
  align-items: stretch;
}

/* ── 卡片 ───────────────────────────────── */
.card {
  position: relative;
  flex: 1;
  min-width: 0;
  padding: 13px 15px 15px 32px;
  border-radius: 18px;
  overflow: hidden;
  box-sizing: border-box;

  background:
    radial-gradient(130% 150% at 0% 8%, color-mix(in srgb, var(--accent) 20%, transparent) 0%, transparent 56%),
    linear-gradient(158deg, rgba(24, 37, 59, 0.88) 0%, rgba(11, 18, 34, 0.92) 52%, rgba(7, 12, 22, 0.96) 100%);
  -webkit-backdrop-filter: blur(16px) saturate(1.15);
  backdrop-filter: blur(16px) saturate(1.15);

  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.1),
    inset 0 -20px 38px -26px color-mix(in srgb, var(--accent) 62%, transparent),
    0 18px 46px -18px color-mix(in srgb, var(--accent) 46%, transparent),
    0 4px 12px rgba(0, 0, 0, 0.55);
}

/* 左上斜切高光，模拟玻璃反光 */
.sheen {
  position: absolute;
  inset: -40% -10% 40% -30%;
  background: linear-gradient(
    104deg,
    transparent 30%,
    rgba(255, 255, 255, 0.07) 45%,
    rgba(255, 255, 255, 0.02) 52%,
    transparent 62%
  );
  pointer-events: none;
}

/* ── 顶行 ───────────────────────────────── */
.topline {
  position: relative;
  display: flex;
  align-items: center;
  gap: 7px;
}

.badge {
  width: 18px;
  height: 18px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: 6px;
  color: #06121d;
  background: color-mix(in srgb, var(--accent) 88%, #ffffff);
  box-shadow: 0 0 10px color-mix(in srgb, var(--accent) 55%, transparent);
}

.agent {
  font-size: 11.5px;
  font-weight: 500;
  letter-spacing: 0.01em;
  color: #d7e2f5;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.time {
  margin-left: auto;
  flex: 0 0 auto;
  font-size: 10.5px;
  color: #6f7f9b;
  font-variant-numeric: tabular-nums;
}

/* ── 大标题 ─────────────────────────────── */
/* 这是参考图的信息主语：状态优先。中文降不到 24px，15px 是 392px 宽下的上限 */
.headline {
  position: relative;
  margin: 9px 0 0;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.25;
  letter-spacing: 0.03em;
  color: #f2f7ff;
  text-shadow: 0 0 14px color-mix(in srgb, var(--accent) 34%, transparent);
}

/* ── 信息行 ─────────────────────────────── */
.meta {
  position: relative;
  margin-top: 9px;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.row {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-size: 11.5px;
  line-height: 1.5;
  min-width: 0;
}

.ic {
  width: 9px;
  height: 9px;
  flex: 0 0 auto;
  align-self: center;
  color: var(--accent);
  filter: drop-shadow(0 0 5px color-mix(in srgb, var(--accent) 70%, transparent));
}

.k {
  flex: 0 0 auto;
  color: #6b7a95;
}

.v {
  min-width: 0;
  color: #b9c7dc;
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  -webkit-box-orient: vertical;
}

/* ── 进度 ───────────────────────────────── */
.bar {
  position: relative;
  margin-top: 10px;
  height: 4px;
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.08);
  overflow: visible;
}

.bar-fill {
  display: block;
  height: 4px;
  border-radius: 3px;
  background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 55%, transparent), var(--accent));
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 75%, transparent);
  transition: width 400ms ease-out;
}

.bar-pct {
  position: absolute;
  right: 0;
  top: -16px;
  font-size: 10px;
  color: #6f7f9b;
  font-variant-numeric: tabular-nums;
}

/* ── 动作行 ─────────────────────────────── */
.acts {
  position: relative;
  margin-top: 11px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.cta-ghost {
  font: inherit;
  font-size: 11.5px;
  padding: 6px 10px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: #7f8ba3;
  cursor: pointer;
  transition: color 140ms ease, background 140ms ease;
}

.cta-ghost:hover {
  color: #c3cee0;
  background: rgba(255, 255, 255, 0.06);
}

/* ── 主按钮 ─────────────────────────────── */
.cta {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  border: 0;
  border-radius: 999px;
  cursor: pointer;
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: #04101a;
  background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 82%, #ffffff), var(--accent));
  box-shadow:
    0 0 0 1px color-mix(in srgb, var(--accent) 70%, transparent),
    0 6px 18px -6px color-mix(in srgb, var(--accent) 80%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.5);
  transition: transform 120ms ease, box-shadow 160ms ease;
}

.cta:hover {
  transform: translateY(-1px);
  box-shadow:
    0 0 0 1px color-mix(in srgb, var(--accent) 85%, transparent),
    0 9px 24px -6px color-mix(in srgb, var(--accent) 95%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.55);
}

.cta:active {
  transform: translateY(0);
}

/* ── 问询 ───────────────────────────────── */
/* 选项直接摆在气泡上：右下角这一张卡就是全部交互，不弹任何窗口。 */
.ask-row {
  position: relative;
  margin-top: 11px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ask-q {
  display: flex;
  align-items: baseline;
  gap: 7px;
  font-size: 11.5px;
  line-height: 1.55;
  color: #b9c7dc;
}

.ask-opts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
}

.ask-opt {
  font: inherit;
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.02em;
  padding: 5px 12px;
  border: 0;
  border-radius: 999px;
  cursor: pointer;
  color: #c3cee0;
  background: rgba(255, 255, 255, 0.07);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12);
  transition: color 140ms ease, background 140ms ease, transform 120ms ease;
}

.ask-opt:hover {
  color: #ffffff;
  background: rgba(255, 255, 255, 0.12);
  transform: translateY(-1px);
}

.ask-opt.primary {
  color: #04101a;
  background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 82%, #ffffff), var(--accent));
  box-shadow:
    0 0 0 1px color-mix(in srgb, var(--accent) 70%, transparent),
    0 6px 18px -6px color-mix(in srgb, var(--accent) 80%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.5);
}

.ask-ignore {
  font: inherit;
  font-size: 11px;
  padding: 5px 9px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: #7f8ba3;
  cursor: pointer;
  transition: color 140ms ease, background 140ms ease;
}

.ask-ignore:hover {
  color: #c3cee0;
  background: rgba(255, 255, 255, 0.06);
}

/* ── 已读 / 渐隐 / 内联全文 ─────────────────── */
.readbtn {
  position: absolute;
  top: 11px;
  right: 13px;
  z-index: 4;
  font: inherit;
  font-size: 10px;
  line-height: 1;
  padding: 3px 8px;
  border: 0;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  color: #8b96ad;
  cursor: pointer;
  opacity: 0;
  transition: opacity 160ms ease, color 140ms ease, background 140ms ease;
}

.card:hover .readbtn,
.glow.hovering .readbtn {
  opacity: 1;
}

.readbtn:hover {
  color: #e8eefb;
  background: rgba(255, 255, 255, 0.12);
}

/* 悬停时时间戳让位给「已读」，信息不丢（时间在详情里也有） */
.card:hover .time {
  opacity: 0;
}

.time {
  transition: opacity 160ms ease;
}

/* 已读倒计时条：滑过后 5 秒从满到空，走完即收起 */
.fadebar {
  position: absolute;
  left: 32px;
  right: 15px;
  bottom: 0;
  height: 2px;
  z-index: 3;
  pointer-events: none;
}

.fadebar i {
  display: block;
  height: 100%;
  width: 100%;
  border-radius: 2px;
  background: var(--accent);
  opacity: 0;
}

.fadebar.on i {
  opacity: 0.55;
  width: 0%;
  transition: width 5s linear;
}

/* 全文内联展开：就在气泡里读完，不弹窗 */
.detail-x {
  position: relative;
  margin-top: 9px;
  padding-top: 8px;
  border-top: 1px dashed rgba(255, 255, 255, 0.12);
  font-size: 11.5px;
  line-height: 1.7;
  color: #c9d6ea;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 190px;
  overflow-y: auto;
}

.detail-x::-webkit-scrollbar {
  width: 6px;
}

.detail-x::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.14);
  border-radius: 3px;
}

/* 三个点按 dididi 的节奏依次亮：听不见的时候，看得见也行 */
.didi {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.didi i {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.28;
  animation: didi 2.4s ease-in-out infinite;
}

.didi i:nth-child(2) {
  animation-delay: 0.16s;
}

.didi i:nth-child(3) {
  animation-delay: 0.32s;
}

.answered {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  margin-top: 11px;
  padding: 5px 12px 5px 8px;
  border-radius: 999px;
  font-size: 11.5px;
  color: #7ff0c4;
  background: rgba(47, 217, 164, 0.1);
  box-shadow: inset 0 0 0 1px rgba(47, 217, 164, 0.3);
}

.answered.failed {
  color: #ff9aa5;
  background: rgba(255, 77, 94, 0.1);
  box-shadow: inset 0 0 0 1px rgba(255, 77, 94, 0.32);
}

.answered.wait {
  color: #e8c07a;
  background: rgba(224, 160, 48, 0.1);
  box-shadow: inset 0 0 0 1px rgba(224, 160, 48, 0.3);
}

/* 圆底与字形用两个颜色。`background: currentColor` 会把字和底涂成同色，
   那个勾实际画出来了但看不见（详情面板里同款问题一并修了）。 */
.tick {
  display: grid;
  place-items: center;
  width: 15px;
  height: 15px;
  flex: none;
  border-radius: 50%;
  font-size: 9px;
  color: #04120d;
  background: #7ff0c4;
}

.answered.failed .tick {
  color: #2a0b0f;
  background: #ff9aa5;
}

.answered.wait .tick {
  color: #2a1d05;
  background: #e8c07a;
}

@keyframes didi {
  0%,
  70%,
  100% {
    opacity: 0.24;
    transform: scale(1);
  }
  8%,
  22% {
    opacity: 1;
    transform: scale(1.5);
  }
}

/* 卡片可点：只有真的有全文/问询时才给指针和悬停反馈，
   否则每条通知都像链接，反而没人敢点。 */
.card.clickable {
  cursor: pointer;
}

.card.clickable:hover {
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 42%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.12),
    inset 0 -20px 38px -26px color-mix(in srgb, var(--accent) 70%, transparent),
    0 20px 50px -18px color-mix(in srgb, var(--accent) 52%, transparent),
    0 4px 12px rgba(0, 0, 0, 0.55);
}

/* ── 角色 ───────────────────────────────── */
.buddy {
  position: absolute;
  left: -11px;
  bottom: -10px;
  width: 150px;
  height: 150px;
  z-index: 2;
  /* robot 自己就是按钮：点它 = 点卡片。光标给手型，可预期。 */
  pointer-events: auto;
  cursor: pointer;
}

/* 环境光斑：让角色像是从自己身上照亮了周围 */
.aura {
  position: absolute;
  inset: 14px;
  border-radius: 50%;
  background: radial-gradient(
    50% 50% at 50% 50%,
    color-mix(in srgb, var(--accent) 50%, transparent) 0%,
    transparent 68%
  );
  filter: blur(16px);
  opacity: 0.5;
}

.buddy-img {
  position: relative;
  width: 150px;
  height: 150px;
  display: block;
  user-select: none;
  /* 透明 alpha 轮廓 → 双层辉光。这就是"透明背景反而是前提" */
  filter:
    drop-shadow(0 0 6px color-mix(in srgb, var(--accent) 62%, transparent))
    drop-shadow(0 0 20px color-mix(in srgb, var(--accent) 34%, transparent));
  transform-origin: bottom center;
  transition: transform 200ms cubic-bezier(0.22, 0.61, 0.36, 1), filter 240ms ease;
}

/* 地面光圈 */
.ring {
  position: absolute;
  left: 50%;
  bottom: 2px;
  width: 124px;
  height: 24px;
  transform: translateX(-50%);
  border-radius: 50%;
  background: radial-gradient(
    50% 50% at 50% 50%,
    color-mix(in srgb, var(--accent) 80%, transparent) 0%,
    color-mix(in srgb, var(--accent) 26%, transparent) 46%,
    transparent 74%
  );
  filter: blur(6px);
}

.hovering .buddy-img {
  transform: translateY(-2px) scale(1.02);
  filter:
    drop-shadow(0 0 9px color-mix(in srgb, var(--accent) 78%, transparent))
    drop-shadow(0 0 28px color-mix(in srgb, var(--accent) 45%, transparent));
}

/* ── 状态差异 ───────────────────────────── */
/* 失败态允许一束更硬的侧光，成功态光更散更柔 */
.is-fail .card {
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 34%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    inset 0 -18px 34px -24px color-mix(in srgb, var(--accent) 70%, transparent),
    0 16px 42px -16px color-mix(in srgb, var(--accent) 60%, transparent),
    0 3px 10px rgba(0, 0, 0, 0.5);
}

.is-idle {
  filter: saturate(0.55);
  opacity: 0.9;
}

/* ── 动效 ───────────────────────────────── */
/* 呼吸：让辉光自己有节奏，这是"活着"最便宜的表达 */
:is(.is-running) .aura {
  animation: ec-glow-pulse 2.6s ease-in-out infinite;
}

:is(.is-running) .ring {
  animation: ec-ring-pulse 2.6s ease-in-out infinite;
}

.breathing {
  animation: ec-breathe 2.4s ease-in-out infinite;
}

/* 兴奋欢呼：带链接的通知，robot 弹跳着喊「快看」。幅度小、频率快，
   与呼吸动画互斥 —— cheering 优先。 */
.cheering {
  animation: ec-cheer 0.9s ease-in-out infinite;
}

/* 受击：点击反馈，短促 squash & stretch。写在 cheering 之后才能盖住它。 */
.buddy-img.poked {
  animation: ec-poke 0.48s cubic-bezier(0.34, 1.56, 0.64, 1);
}

@keyframes ec-poke {
  0% {
    transform: scale(1);
  }
  35% {
    transform: scale(0.9) rotate(-3deg);
  }
  70% {
    transform: scale(1.06) rotate(2deg);
  }
  100% {
    transform: scale(1);
  }
}

/* 点击反馈行：浮在卡片底部，出现/离开都轻 */
.flashline {
  position: absolute;
  left: 32px;
  right: 15px;
  bottom: 8px;
  z-index: 6;
  padding: 4px 10px;
  border-radius: 8px;
  font-size: 10.5px;
  line-height: 1.4;
  text-align: center;
  color: #ffe3a6;
  background: rgba(8, 13, 24, 0.92);
  box-shadow:
    inset 0 0 0 1px rgba(232, 192, 122, 0.35),
    0 4px 14px rgba(0, 0, 0, 0.45);
  pointer-events: none;
}

.flashpop-enter-active,
.flashpop-leave-active {
  transition: opacity 160ms ease, transform 160ms ease;
}

.flashpop-enter-from,
.flashpop-leave-to {
  opacity: 0;
  transform: translateY(4px);
}

/* ── 交付清单弹层 ─────────────────────────── */
/* 铺在卡片下部：玻璃底 + accent 描边，与气泡同语言。
   最高 5 行出滚动，多了反而挑不动。 */
.delivery {
  position: absolute;
  left: 32px;
  right: 12px;
  bottom: 10px;
  z-index: 7;
  border-radius: 14px;
  overflow: hidden;
  background:
    radial-gradient(120% 140% at 0% 0%, color-mix(in srgb, var(--accent) 14%, transparent) 0%, transparent 52%),
    linear-gradient(158deg, rgba(18, 28, 46, 0.97) 0%, rgba(9, 15, 28, 0.98) 100%);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 32%, transparent),
    0 14px 34px -12px rgba(0, 0, 0, 0.7);
}

.d-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px 7px 13px;
  font-size: 10.5px;
  letter-spacing: 0.04em;
  color: #9fb0ca;
  border-bottom: 1px solid rgba(255, 255, 255, 0.07);
}

.d-close {
  font: inherit;
  font-size: 10px;
  line-height: 1;
  padding: 3px 7px;
  border: 0;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  color: #8b96ad;
  cursor: pointer;
  transition: color 140ms ease, background 140ms ease;
}

.d-close:hover {
  color: #e8eefb;
  background: rgba(255, 255, 255, 0.12);
}

.d-list {
  max-height: 168px;
  overflow-y: auto;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.d-list::-webkit-scrollbar {
  width: 6px;
}

.d-list::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.14);
  border-radius: 3px;
}

.d-row {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 9px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 130ms ease, transform 120ms ease;
}

.d-row:hover {
  background: rgba(255, 255, 255, 0.07);
  transform: translateX(2px);
}

.d-row:active {
  transform: translateX(2px) scale(0.99);
}

.d-ic {
  flex: none;
  font-size: 14px;
  line-height: 1;
  filter: drop-shadow(0 0 6px color-mix(in srgb, var(--accent) 45%, transparent));
}

.d-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.d-name {
  font-size: 11.5px;
  font-weight: 500;
  color: #dce7f8;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.d-sub {
  font-size: 10px;
  color: #6f7f9b;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.d-go {
  flex: none;
  font-size: 11px;
  color: var(--accent);
  opacity: 0.65;
}

.d-row:hover .d-go {
  opacity: 1;
}

.dpop-enter-active,
.dpop-leave-active {
  transition: opacity 150ms ease, transform 150ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

.dpop-enter-from,
.dpop-leave-to {
  opacity: 0;
  transform: translateY(8px) scale(0.98);
}

/* 附件行可点：整行是「预览全部」的入口 */
.row.openable {
  cursor: pointer;
  border-radius: 6px;
  margin: 0 -4px;
  padding: 1px 4px;
  transition: background 130ms ease;
}

.row.openable:hover {
  background: rgba(255, 255, 255, 0.05);
}

.row.openable:hover .v {
  color: #d8e4f6;
}

@keyframes ec-cheer {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }
  30% {
    transform: translateY(-6px) scale(1.04);
  }
  55% {
    transform: translateY(0) scale(0.99);
  }
  70% {
    transform: translateY(-3px) scale(1.02);
  }
}

@keyframes ec-glow-pulse {
  0%,
  100% {
    opacity: 0.45;
    transform: scale(1);
  }
  50% {
    opacity: 0.78;
    transform: scale(1.07);
  }
}

@keyframes ec-ring-pulse {
  0%,
  100% {
    opacity: 0.7;
    transform: translateX(-50%) scaleX(1);
  }
  50% {
    opacity: 1;
    transform: translateX(-50%) scaleX(1.1);
  }
}

@keyframes ec-breathe {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(1.035);
  }
}

@media (prefers-reduced-motion: reduce) {
  .buddy-img,
  .aura,
  .ring,
  .cta,
  .ask-opt,
  .didi i,
  .fadebar i {
    animation: none !important;
    transition: none !important;
  }
  .didi i {
    opacity: 1;
  }
  .aura {
    opacity: 0.55;
  }
  /* 倒计时条不做动画时直接隐藏：没有「正在倒计时」的视觉，就不该收走 */
  .fadebar.on i {
    width: 100%;
  }
}
</style>
