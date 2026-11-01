<script setup lang="ts">
import { computed, ref } from "vue";
import { useBuddy } from "../buddies/useBuddy";
import type { BuddyAction } from "../buddies/types";
import type { AnswerRecord, MediaItem } from "../types/notice";

type GlowState = "success" | "running" | "fail" | "idle";

interface GlowAction {
  id: string;
  label: string;
  primary?: boolean;
}

interface GlowAsk {
  question: string;
  options: { id: string; label: string; primary?: boolean }[];
}

/** 动作交给上层处理：组件不知道「重试」到底该 invoke 什么。 */
const emit = defineEmits<{ action: [id: string]; open: [] }>();

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

const accentColor = computed(() => props.accent ?? ACCENT[props.state]);
const headlineText = computed(() => props.headline ?? HEADLINE[props.state]);

const settled = computed(() => props.state === "success" || props.state === "fail");
/// 只有包里真的有 running 帧才播呼吸，否则静止比错帧好
const breathing = computed(() => props.state === "running" && buddy.supports("running"));
const currentSrc = computed(() => buddy.src(props.state as BuddyAction));
const progressPct = computed(() =>
  props.progress === undefined ? null : Math.round(props.progress * 100)
);

/** 有全文可看、有问询、或带了媒体附件，卡片就该能点开 —— 否则「查看结果」永远是死按钮。 */
const openable = computed(
  () => Boolean(props.detail) || Boolean(props.ask) || (props.media?.length ?? 0) > 0
);

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
    @mouseenter="hovering = true"
    @mouseleave="hovering = false"
  >
    <!-- 角色：压在卡片左缘，半身在卡片之外 -->
    <div class="buddy">
      <span class="aura" aria-hidden="true" />
      <img :src="currentSrc" class="buddy-img" :class="{ breathing }" alt="" draggable="false" />
      <span class="ring" aria-hidden="true" />
    </div>

    <div
      class="card"
      :class="{ clickable: openable }"
      :title="openable ? '点开看完整内容' : undefined"
      @click="openable && emit('open')"
    >
      <span class="sheen" aria-hidden="true" />

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
          <span class="v">{{ result }}</span>
        </div>

        <div v-if="mediaCount > 0" class="row">
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
          <span class="v">📎 {{ mediaCount }} 个文件</span>
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

      <!-- ── 问询：占掉动作区的位置 ─────────────
           有问询时不再显示动作按钮。两排按钮会让人分不清「哪个是回答问题」，
           而回答问题才是这条通知唯一该做的事。 -->
      <div v-if="ask && !answered" class="ask-row">
        <button class="ask-cta" @click.stop="emit('open')">
          <span class="didi" aria-hidden="true"><i /><i /><i /></span>
          <span class="ask-label">等你答复…</span>
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
/* 整块做成一个「在等你」的按钮：不是选项，是入口。选项在面板里，
   因为 400px 宽的气泡放不下问题本身 —— 只放选项就成了盲选。 */
.ask-row {
  position: relative;
  margin-top: 11px;
}

.ask-cta {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 13px 6px 11px;
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.02em;
  border-radius: 999px;
  cursor: pointer;
  color: color-mix(in srgb, var(--accent) 92%, #ffffff);
  border: 1px solid color-mix(in srgb, var(--accent) 52%, transparent);
  background: color-mix(in srgb, var(--accent) 15%, rgba(8, 14, 26, 0.6));
  box-shadow:
    0 0 0 0 color-mix(in srgb, var(--accent) 40%, transparent),
    0 6px 20px -10px color-mix(in srgb, var(--accent) 80%, transparent);
  transition: background 160ms ease, transform 120ms ease;
  /* 呼吸一圈，频率跟 dididi 的三连对齐：不用再响一次也能看出它在等 */
  animation: ask-breathe 2.4s ease-in-out infinite;
}

.ask-cta:hover {
  background: color-mix(in srgb, var(--accent) 26%, rgba(8, 14, 26, 0.6));
  transform: translateY(-1px);
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

@keyframes ask-breathe {
  0%,
  100% {
    box-shadow:
      0 0 0 0 color-mix(in srgb, var(--accent) 34%, transparent),
      0 6px 20px -10px color-mix(in srgb, var(--accent) 70%, transparent);
  }
  50% {
    box-shadow:
      0 0 0 4px color-mix(in srgb, var(--accent) 0%, transparent),
      0 8px 24px -10px color-mix(in srgb, var(--accent) 90%, transparent);
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
  pointer-events: none;
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
  .ask-cta,
  .didi i {
    animation: none !important;
    transition: none !important;
  }
  .didi i {
    opacity: 1;
  }
  .aura {
    opacity: 0.55;
  }
}
</style>
