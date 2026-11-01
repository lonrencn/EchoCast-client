<script setup lang="ts">
import { computed } from "vue";
import TopBarSegment from "./TopBarSegment.vue";
import MuteButton from "./MuteButton.vue";
import { useBuddy } from "../buddies/useBuddy";
import type { BuddyAction } from "../buddies/types";
import { notices, answeredFor } from "../feed/notices";
import { toBubbleView, type BubbleView } from "../types/adapt";

/**
 * 顶部灯条。
 *
 * 和右下角气泡是同一个数据源的两种画法，不是两套 pipeline：
 * 组件都只吃 `BubbleView`，服务端一个字都不用改。
 *
 * 几条硬约束（都是被「一行」这个形状逼出来的）：
 * - **只有一行**。再多条也压在同一行里，超过上限折叠成「+N」，
 *   绝不长高 —— 灯条一旦长高就变成面板，就不该挂在屏幕最上沿了。
 * - **最多并排两条**。1920 宽下三条各分到 600px 还算能看，但 1366 的笔记本
 *   只剩 430px，执行方和结果都会被截成省略号，读起来比「+1」更费劲。
 *   这和气泡的 3 条上限不冲突：气泡是竖着堆的，宽度不吃紧。
 * - **一条灯条只有一个主色**。颜色取最前面那条（最新、最该被看见）。
 *   每条各染一个色会变成跑马灯，反而看不出哪条要紧。
 * - **没有通知就整条收起来**。一条常驻的空灯条等于屏幕顶部多了一个黑边。
 */
interface BarRow {
  id: string;
  view: BubbleView;
}

const props = withDefaults(
  defineProps<{
    /** 演示/预览时直接给数据；不传则读全局 feed。 */
    items?: BarRow[];
    /** fixed = 贴窗口顶边；inline = 页内嵌一条，用来看设计。 */
    position?: "fixed" | "inline";
    max?: number;
    /** 右上角的齿轮，点开设置。inline 演示里不显示。 */
    gear?: boolean;
  }>(),
  { position: "fixed", max: 2, gear: true }
);

const emit = defineEmits<{
  action: [id: string, actionId: string];
  settings: [];
  sheet: [id: string];
}>();

const rows = computed<BarRow[]>(() => {
  const all: BarRow[] = props.items
    ? props.items
    : notices.value.map((n) => ({
        id: n.id,
        view: toBubbleView(n, answeredFor(n.id)),
      }));
  return all.slice(0, props.max);
});

const lead = computed(() => rows.value[0] ?? null);

/**
 * 折叠条数。必须拿**完整列表**去算：feed 里的 `overflow` 是按气泡的 3 条上限
 * 算的，灯条上限更小，直接复用会少数一条。
 */
const rest = computed(() => {
  const total = props.items ? props.items.length : notices.value.length;
  return Math.max(0, total - rows.value.length);
});

const show = computed(() => props.position === "inline" || rows.value.length > 0);

const ACCENT: Record<string, string> = {
  success: "#2FD9A4",
  running: "#4DA6FF",
  fail: "#FF4D5E",
  idle: "#8B93A7",
};

const accent = computed(
  () => lead.value?.view.accent ?? ACCENT[lead.value?.view.state ?? "idle"] ?? ACCENT.idle
);

/** 有任务在跑时灯带才流动。全都落定了还动，就是在骗人还在忙。 */
const live = computed(() => rows.value.some((r) => r.view.state === "running"));

/** 灯条左侧那一个形象。它跟随最新一条的包，逐条各放一个在 46px 里就是一堆色块。 */
const buddy = useBuddy(() => lead.value?.view.pack);
const faceSrc = computed(() =>
  lead.value ? buddy.src(lead.value.view.state as BuddyAction) : ""
);
const breathing = computed(
  () => lead.value?.view.state === "running" && buddy.supports("running")
);
</script>

<template>
  <Transition name="drop">
    <div
      v-if="show"
      class="bar"
      :class="[`at-${position}`, { live }]"
      :style="{ '--accent': accent }"
      role="status"
      aria-live="polite"
    >
      <!-- 灯带：整条唯一一条“光”，颜色跟随最上面那条通知 -->
      <span class="rail" aria-hidden="true"><i /></span>

      <div v-if="faceSrc" class="cap" aria-hidden="true">
        <span class="aura" />
        <img
          :src="faceSrc"
          class="face"
          :class="{ breathing }"
          alt=""
          draggable="false"
        />
      </div>
      <span v-else class="cap-cap" aria-hidden="true" />

      <TransitionGroup name="slide" tag="div" class="rows">
        <TopBarSegment
          v-for="r in rows"
          :key="r.id"
          :view="r.view"
          :dense="rows.length > 1"
          @action="(a: string) => emit('action', r.id, a)"
          @sheet="emit('sheet', r.id)"
        />
      </TransitionGroup>

      <!-- 折叠数紧跟内容，不甩到屏幕另一头 —— 离得太远就看不出它在说哪几条 -->
      <span v-if="rest > 0" class="more" :title="`还有 ${rest} 条`">+{{ rest }}</span>

      <span class="spring" />

      <!-- 喇叭放在齿轮左边：最常用的两个入口挨在一起，右端只是一条工具栏，
           不抢通知内容的横向空间。灯条收起时没有这个按钮，靠快捷键兜底。 -->
      <MuteButton v-if="position === 'fixed'" size="sm" />

      <button
        v-if="gear && position === 'fixed'"
        class="gear"
        aria-label="设置"
        @click="emit('settings')"
      >
        ⚙
      </button>
    </div>
  </Transition>
</template>

<style scoped>
.bar {
  box-sizing: border-box;
  /* 46 -> 54：给小机器人留出一整只身体的高度，而不是只露出个头 */
  height: 54px;
  display: flex;
  align-items: stretch;
  overflow: hidden;
  padding: 0 6px 0 4px;

  background: linear-gradient(180deg, rgba(13, 20, 34, 0.94) 0%, rgba(7, 11, 20, 0.9) 100%);
  -webkit-backdrop-filter: blur(18px) saturate(1.2);
  backdrop-filter: blur(18px) saturate(1.2);
  box-shadow:
    inset 0 -1px 0 rgba(255, 255, 255, 0.06),
    0 10px 30px -22px rgba(0, 0, 0, 0.95);
}

.at-fixed {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 60;
}

/* 页内演示：同一条灯条，只是不贴着窗口边，四角收圆 */
.at-inline {
  position: relative;
  border-radius: 14px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  box-shadow:
    inset 0 -1px 0 rgba(255, 255, 255, 0.05),
    0 14px 34px -24px rgba(0, 0, 0, 0.9);
}

/* ── 灯带 ───────────────────────────────── */
.rail {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 2px;
  overflow: hidden;
  opacity: 0.8;
  background: linear-gradient(
    90deg,
    transparent 0%,
    color-mix(in srgb, var(--accent) 55%, transparent) 10%,
    color-mix(in srgb, var(--accent) 78%, transparent) 52%,
    transparent 100%
  );
}

/* 流动的那一束：只在有任务跑的时候出现 */
.rail i {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 22%;
  opacity: 0;
  background: linear-gradient(
    90deg,
    transparent,
    color-mix(in srgb, var(--accent) 40%, #ffffff) 50%,
    transparent
  );
}

.live .rail i {
  opacity: 0.55;
  animation: rail-sweep 2.6s cubic-bezier(0.5, 0, 0.5, 1) infinite;
}

/* ── 形象 ───────────────────────────────── */
.cap {
  position: relative;
  flex: none;
  /* 机器人是白身子，30px 时只剩一团灰；给到 36px 才看得出是只机器人 */
  width: 58px;
  display: grid;
  place-items: center;
}

.cap::after {
  content: "";
  position: absolute;
  right: 2px;
  top: 50%;
  width: 1px;
  height: 16px;
  transform: translateY(-50%);
  background: linear-gradient(180deg, transparent, rgba(255, 255, 255, 0.12), transparent);
}

.aura {
  position: absolute;
  width: 46px;
  height: 46px;
  border-radius: 50%;
  background: radial-gradient(
    50% 50% at 50% 50%,
    color-mix(in srgb, var(--accent) 55%, transparent) 0%,
    transparent 70%
  );
  filter: blur(7px);
  opacity: 0.65;
}

.face {
  position: relative;
  width: 44px;
  height: 44px;
  object-fit: contain;
  display: block;
  user-select: none;
  filter:
    drop-shadow(0 0 5px color-mix(in srgb, var(--accent) 60%, transparent))
    drop-shadow(0 0 14px color-mix(in srgb, var(--accent) 30%, transparent));
}

.breathing {
  animation: bar-breathe 2.6s ease-in-out infinite;
}

/* 没有形象（包缺图）时的空位，宽度不变，避免整条左右跳 */
.cap-cap {
  flex: none;
  width: 12px;
}

/* ── 分段容器 ───────────────────────────── */
.rows {
  display: flex;
  align-items: stretch;
  min-width: 0;
  flex: 0 1 auto;
}

.spring {
  flex: 1 1 auto;
  min-width: 0;
}

/* ── 右端 ───────────────────────────────── */
.more {
  flex: none;
  align-self: center;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 10.5px;
  letter-spacing: 0.02em;
  color: #8e9cb5;
  background: rgba(255, 255, 255, 0.05);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
}

.gear {
  flex: none;
  align-self: center;
  width: 24px;
  height: 24px;
  margin-left: 6px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  font-size: 11px;
  line-height: 1;
  color: #7c8aa2;
  background: transparent;
  cursor: pointer;
  transition: background 140ms ease, color 140ms ease;
}

.gear:hover {
  color: #eaf2ff;
  background: rgba(255, 255, 255, 0.08);
}

/* ── 动效 ───────────────────────────────── */
/* 整条从顶上垂下来：既解释了它挂在哪，也不会从中间“长”出来压到内容 */
.drop-enter-active {
  transition: transform 220ms cubic-bezier(0.22, 0.61, 0.36, 1), opacity 160ms ease;
}

.drop-leave-active {
  transition: transform 180ms cubic-bezier(0.4, 0, 0.7, 0.3), opacity 140ms ease;
}

.drop-enter-from,
.drop-leave-to {
  opacity: 0;
  transform: translateY(-102%);
}

.slide-enter-active {
  transition: opacity 180ms ease, transform 200ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

.slide-leave-active {
  transition: opacity 120ms ease, transform 120ms ease;
}

.slide-enter-from {
  opacity: 0;
  transform: translateY(-5px);
}

.slide-leave-to {
  opacity: 0;
  transform: scaleX(0.96);
}

.slide-move {
  transition: transform 240ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

@keyframes rail-sweep {
  0% {
    transform: translateX(-110%);
  }
  100% {
    transform: translateX(560%);
  }
}

@keyframes bar-breathe {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(1.06);
  }
}

@media (prefers-reduced-motion: reduce) {
  .drop-enter-active,
  .drop-leave-active,
  .slide-enter-active,
  .slide-leave-active,
  .slide-move {
    transition-duration: 1ms;
  }
  .rail i,
  .face {
    animation: none !important;
  }
}
</style>
