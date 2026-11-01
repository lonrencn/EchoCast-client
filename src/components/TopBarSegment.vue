<script setup lang="ts">
import { computed, ref } from "vue";
import type { BubbleView } from "../types/adapt";

/**
 * 顶部灯条里的一段。
 *
 * 和气泡的区别是**只能横着长**：高度锁死在灯条里，所有信息压成一行，
 * 谁先被牺牲是有顺序的 —— 时间 → 结果 → 次要动作 → 执行方。
 * 顺序反过来就会先丢掉任务名，那一段就彻底读不懂了。
 */
const emit = defineEmits<{ action: [id: string]; sheet: []; dismiss: [] }>();

const props = defineProps<{
  view: BubbleView;
  /** 一条灯条上有多段时的紧凑档：先砍掉时间这类「可以事后查」的信息。 */
  dense?: boolean;
}>();

const ACCENT: Record<string, string> = {
  success: "#2FD9A4",
  running: "#4DA6FF",
  fail: "#FF4D5E",
  idle: "#8B93A7",
};

const accent = computed(() => props.view.accent ?? ACCENT[props.view.state] ?? ACCENT.idle);
const pct = computed(() =>
  props.view.progress === undefined ? null : Math.round(props.view.progress * 100)
);
const primary = computed(() => props.view.actions[0] ?? null);
const secondary = computed(() => props.view.actions[1] ?? null);
const hovering = ref(false);

/** 问询且还没答复：这一段的动作区换成催办。灯条上放不下问题，所以只能点开。 */
const asking = computed(() => Boolean(props.view.ask) && !props.view.answered);

/** 整行悬停时把被截断的内容显出来，不必点开任何东西。 */
const hint = computed(() =>
  [props.view.executor, props.view.task, props.view.result].filter(Boolean).join(" · ")
);
</script>

<template>
  <div
    class="seg"
    :class="[`is-${view.state}`, { hovering, dense }]"
    :style="{ '--accent': accent }"
    :title="hint"
    @click="emit('dismiss')"
    @mouseenter="hovering = true"
    @mouseleave="hovering = false"
  >
    <span class="chip">
      <i class="led" aria-hidden="true" />
      {{ view.badge }}
    </span>

    <span class="who">
      {{ view.executorShort }}
      <em v-if="view.node" class="node">· {{ view.node }}</em>
    </span>
    <span class="slash" aria-hidden="true">·</span>
    <span class="task">{{ view.task }}</span>

    <template v-if="view.result">
      <span class="arrow" aria-hidden="true">→</span>
      <span class="res">{{ view.result }}</span>
    </template>

    <span v-if="pct !== null" class="pct">{{ pct }}%</span>

    <!-- 问询优先于动作：一条灯条上最该被看见的是「轮到你说话了」 -->
    <button v-if="asking" class="ask" @click.stop="emit('sheet')">
      <span class="didi" aria-hidden="true"><i /><i /><i /></span>
      等你答复…
    </button>
    <span v-else-if="view.answered" class="done" :class="{ failed: view.answered.failed }">
      {{ view.answered.failed ? "没发出去" : `已答复 · ${view.answered.label}` }}
    </span>
    <template v-else>
      <button
        v-if="primary"
        class="cta"
        @click.stop="primary && emit('action', primary.id)"
      >
        {{ primary.label }}
      </button>
      <button
        v-if="secondary"
        class="ghost"
        @click.stop="secondary && emit('action', secondary.id)"
      >
        {{ secondary.label }}
      </button>
    </template>

    <span class="time">{{ view.time }}</span>

    <!-- Windows 习惯的 ✕：点一下就关掉这段，不用解释、不用学习 -->
    <button class="x" aria-label="关闭这条通知" title="关闭" @click.stop="emit('dismiss')">
      <svg viewBox="0 0 10 10" width="9" height="9" aria-hidden="true">
        <path
          d="M1.8 1.8 8.2 8.2M8.2 1.8 1.8 8.2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
        />
      </svg>
    </button>

    <!-- 进度不另起一条：直接点亮本段底部那根发丝线 -->
    <span v-if="pct !== null" class="prog" aria-hidden="true">
      <i :style="{ width: `${pct}%` }" />
    </span>
  </div>
</template>

<style scoped>
.seg {
  position: relative;
  flex: 0 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 0 14px;
  box-sizing: border-box;
  transition: background 140ms ease;
  /* 点一段 = 收起这条通知（Windows 的「点窗口就没了」直觉） */
  cursor: pointer;
}

.seg + .seg::before {
  content: "";
  position: absolute;
  left: 0;
  top: 50%;
  width: 1px;
  height: 18px;
  transform: translateY(-50%);
  background: linear-gradient(180deg, transparent, rgba(255, 255, 255, 0.12), transparent);
}

.seg.hovering {
  background: rgba(255, 255, 255, 0.035);
}

/* ── 状态 ───────────────────────────────── */
.chip {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 7px 2px 6px;
  border-radius: 5px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.04em;
  white-space: nowrap;
  color: color-mix(in srgb, var(--accent) 92%, #ffffff);
  background: color-mix(in srgb, var(--accent) 15%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent);
}

.led {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 6px var(--accent);
}

.is-running .led {
  animation: bar-led 1.4s ease-in-out infinite;
}

/* ── 文本 ───────────────────────────────── */
.who {
  /* 名字不参与压缩：哪怕只少 5px，也会变成「Soun…」这种读不出来的断口。
     真名字本身太长时，用 max-width 封顶再省略，代价可控。 */
  flex: 0 0 auto;
  max-width: 8.5em;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  color: #8e9cb5;
}

/* 节点名是补充信息：一条灯条挤两条时整块丢掉，
   让「Soundao」完整显示，而不是留个「Soundao · Ed…」的断口 */
.node {
  font-style: normal;
  font-size: 10.5px;
  color: #6b7a94;
}

.dense .node {
  display: none;
}

.slash {
  flex: none;
  font-size: 11px;
  color: #46536b;
}

.task {
  /* 任务名最长、也最有得截：让它先吸收压缩，短名字才不会被砍成「Soun…」 */
  flex: 0 3 auto;
  min-width: 4.5em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 500;
  letter-spacing: 0.01em;
  color: #e6eefb;
}

.arrow {
  flex: none;
  font-size: 11px;
  color: color-mix(in srgb, var(--accent) 70%, #ffffff);
  opacity: 0.75;
}

.res {
  flex: 0 2 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  color: #a8b6cc;
}

.pct {
  flex: none;
  font-size: 10px;
  color: color-mix(in srgb, var(--accent) 80%, #ffffff);
  font-variant-numeric: tabular-nums;
}

.time {
  flex: none;
  margin-left: auto;
  padding-left: 4px;
  font-size: 10px;
  color: #6b7a94;
  font-variant-numeric: tabular-nums;
}

/* ✕ 关闭钮：平时低调，悬停浮现 —— 和气泡的「已读」同一个语言 */
.x {
  flex: none;
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: #71809c;
  cursor: pointer;
  opacity: 0;
  transition: opacity 140ms ease, color 140ms ease, background 140ms ease;
}

.seg.hovering .x {
  opacity: 1;
}

.x:hover {
  color: #ffb3ba;
  background: rgba(255, 77, 94, 0.12);
}

/* ── 动作 ───────────────────────────────── */
.cta {
  flex: none;
  margin-left: 2px;
  padding: 3px 10px;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  border: 0;
  border-radius: 999px;
  cursor: pointer;
  color: #04101a;
  background: linear-gradient(
    180deg,
    color-mix(in srgb, var(--accent) 80%, #ffffff),
    var(--accent)
  );
  box-shadow:
    0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent),
    0 4px 12px -6px color-mix(in srgb, var(--accent) 85%, transparent);
  transition: filter 120ms ease, transform 120ms ease;
}

.cta:hover {
  filter: brightness(1.1);
  transform: translateY(-1px);
}

.cta:active {
  transform: translateY(0);
}

.ghost {
  flex: none;
  padding: 3px 8px;
  font: inherit;
  font-size: 11px;
  border: 0;
  border-radius: 999px;
  cursor: pointer;
  background: transparent;
  color: #7f8ba3;
  transition: color 140ms ease, background 140ms ease;
}

.ghost:hover {
  color: #c3cee0;
  background: rgba(255, 255, 255, 0.07);
}

/* ── 问询 ───────────────────────────────── */
/* 灯条只有一行，问题本身放不下 —— 这里只负责「让人知道该点」。
   所以它不是按钮的样式，而是一个会呼吸的提醒。 */
.ask {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-left: 2px;
  padding: 3px 10px 3px 9px;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  border-radius: 999px;
  cursor: pointer;
  color: color-mix(in srgb, var(--accent) 92%, #ffffff);
  border: 1px solid color-mix(in srgb, var(--accent) 50%, transparent);
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  animation: seg-ask 2.4s ease-in-out infinite;
}

.ask:hover {
  background: color-mix(in srgb, var(--accent) 28%, transparent);
}

.didi {
  display: inline-flex;
  align-items: center;
  gap: 2.5px;
}

.didi i {
  width: 3.5px;
  height: 3.5px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.25;
  animation: seg-didi 2.4s ease-in-out infinite;
}

.didi i:nth-child(2) {
  animation-delay: 0.16s;
}

.didi i:nth-child(3) {
  animation-delay: 0.32s;
}

.done {
  flex: none;
  margin-left: 2px;
  padding: 3px 9px;
  font-size: 10.5px;
  white-space: nowrap;
  border-radius: 999px;
  color: #7ff0c4;
  background: rgba(47, 217, 164, 0.1);
  box-shadow: inset 0 0 0 1px rgba(47, 217, 164, 0.28);
}

.done.failed {
  color: #ff9aa5;
  background: rgba(255, 77, 94, 0.1);
  box-shadow: inset 0 0 0 1px rgba(255, 77, 94, 0.3);
}

@keyframes seg-didi {
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

@keyframes seg-ask {
  0%,
  100% {
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 30%, transparent);
  }
  50% {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 0%, transparent);
  }
}

/* ── 进度 ───────────────────────────────── */
.prog {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 2px;
  overflow: hidden;
}

.prog i {
  display: block;
  height: 100%;
  background: var(--accent);
  box-shadow: 0 0 10px color-mix(in srgb, var(--accent) 90%, transparent);
  transition: width 420ms ease-out;
}

/* ── 空间不够时的降级 ───────────────────────
   按「信息价值」倒序砍：时间最先走，其次结果，再其次次要动作。
   任务名和执行方任何时候都留着 —— 少了它们，这一段就只剩一个颜色。

   不用容器查询：`container-type: inline-size` 会给 flex 项加上行内尺寸包含，
   内在宽度直接按 0 算，分段会塌成一条缝。宁可用确定性的两档。 */
.dense .time {
  display: none;
}

.dense .ghost {
  display: none;
}

@media (max-width: 900px) {
  .time {
    display: none;
  }
  .res,
  .arrow {
    display: none;
  }
}

@media (max-width: 720px) {
  .ghost,
  .pct,
  .who,
  .slash {
    display: none;
  }
  .seg {
    padding: 0 10px;
  }
  .seg + .seg::before {
    display: none;
  }
}

@keyframes bar-led {
  50% {
    opacity: 0.35;
  }
}

.is-running .prog i {
  animation: bar-flow 2.2s ease-in-out infinite;
}

@keyframes bar-flow {
  50% {
    filter: brightness(1.5);
  }
}

@media (prefers-reduced-motion: reduce) {
  .led,
  .prog i,
  .ask,
  .didi i {
    animation: none !important;
  }
  .didi i {
    opacity: 1;
  }
  .prog i {
    transition-duration: 1ms;
  }
}
</style>
