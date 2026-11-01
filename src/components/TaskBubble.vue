<script setup lang="ts">
import { computed, ref } from "vue";
import { useBuddy } from "../buddies/useBuddy";
import type { BuddyAction } from "../buddies/types";

type BubbleState = "success" | "fail" | "running" | "idle";

interface BubbleAction {
  id: string;
  label: string;
  primary?: boolean;
}

const props = withDefaults(
  defineProps<{
    state?: BubbleState;
    title: string;
    executor: string;
    result?: string;
    time?: string;
    actions?: BubbleAction[];
    progress?: number;
    /** 指定素材包 id。留空则跟随全局设置 */
    pack?: string;
  }>(),
  {
    state: "success",
    result: "",
    time: "刚刚",
    actions: () => [],
  }
);

const hovering = ref(false);

const buddy = useBuddy(() => props.pack);

/// 成功与失败都是"落定态"：不呼吸、不眨眼、悬停也不理你 —— 情绪要停住
const settled = computed(() => props.state === "success" || props.state === "fail");

const baseSrc = computed(() => buddy.src(props.state as BuddyAction));

/// 包里有 hover 帧才切图，否则保持原帧，靠位移表达
const currentSrc = computed(() =>
  hovering.value && !settled.value && buddy.supports("hover")
    ? buddy.src("hover")
    : baseSrc.value
);

/// 眨眼是一张独立叠加帧，包不提供就不播
const blinkSrc = computed(() => buddy.src("idle_blink"));
const canBlink = computed(() => buddy.supports("idle_blink"));

const breathing = computed(() => !settled.value && !hovering.value);
const progressPct = computed(() =>
  props.progress === undefined ? null : Math.round(props.progress * 100)
);
</script>

<template>
  <div
    class="bubble"
    :class="[`is-${state}`, { hovering }]"
    @mouseenter="hovering = true"
    @mouseleave="hovering = false"
  >
    <div class="strip" />

    <div class="body">
      <div class="head">
        <span class="dot" />
        <span class="title">{{ title }}</span>
        <span class="time">{{ time }}</span>
      </div>

      <div class="executor">{{ executor }}</div>

      <div v-if="progressPct !== null" class="progress">
        <div class="track">
          <div class="fill" :style="{ width: `${progressPct}%` }" />
        </div>
        <span class="pct">{{ progressPct }}%</span>
      </div>

      <div v-if="result" class="result">{{ result }}</div>

      <div v-if="actions.length" class="actions">
        <button
          v-for="a in actions"
          :key="a.id"
          class="act"
          :class="{ primary: a.primary }"
        >
          {{ a.label }}
        </button>
      </div>
    </div>

    <div class="buddy-wrap">
      <img
        :src="currentSrc"
        class="buddy base"
        :class="{ breathing }"
        alt=""
        draggable="false"
      />
      <img
        v-if="breathing && canBlink"
        :src="blinkSrc"
        class="buddy blink"
        alt=""
        draggable="false"
      />
    </div>
  </div>
</template>

<style scoped>
.bubble {
  --bg: #fbf2e4;
  --ink: #3b2a1e;
  --ink-2: #5c4634;
  --muted: #8a7460;
  --accent: #b26a45;
  --line: rgba(59, 42, 30, 0.12);

  position: relative;
  width: 352px;
  max-width: 100%;
  box-sizing: border-box;
  background: var(--bg);
  border: 1px solid var(--line);
  border-radius: 12px;
  overflow: hidden;
}

.is-running {
  --accent: #ba7517;
}

.is-fail {
  --accent: #a34a3a;
  border-left: 2px solid #a34a3a;
}

.strip {
  height: 2px;
  background: var(--accent);
}

.body {
  padding: 11px 13px 12px;
}

.head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.dot {
  width: 5px;
  height: 5px;
  flex: 0 0 auto;
  border-radius: 50%;
  background: var(--accent);
}

.title {
  font-family: "Noto Serif SC", "Songti SC", "SimSun", serif;
  font-size: 13px;
  font-weight: 500;
  color: var(--ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}

.time {
  margin-left: auto;
  flex: 0 0 auto;
  font-size: 11px;
  color: var(--muted);
}

.executor {
  margin: 3px 0 0 13px;
  font-size: 11px;
  color: var(--muted);
}

.result {
  margin: 7px 46px 0 13px;
  font-size: 12px;
  line-height: 1.55;
  color: var(--ink-2);
}

.progress {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 7px 46px 0 13px;
}

.track {
  flex: 1;
  height: 3px;
  border-radius: 2px;
  background: rgba(186, 117, 23, 0.18);
  overflow: hidden;
}

.fill {
  height: 3px;
  border-radius: 2px;
  background: var(--accent);
  transition: width 400ms ease-out;
}

.pct {
  font-size: 11px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}

.actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 9px 0 0 13px;
}

.act {
  font: inherit;
  font-size: 12px;
  padding: 3px 9px;
  border-radius: 6px;
  border: 1px solid var(--line);
  background: transparent;
  color: var(--ink-2);
  cursor: pointer;
  transition: background 120ms ease, border-color 120ms ease;
}

.act:hover {
  border-color: rgba(59, 42, 30, 0.3);
}

.act.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

.act.primary:hover {
  filter: brightness(1.08);
}

.buddy-wrap {
  position: absolute;
  right: 12px;
  bottom: 10px;
  width: 40px;
  height: 40px;
  pointer-events: none;
}

.buddy {
  position: absolute;
  inset: 0;
  width: 40px;
  height: 40px;
  display: block;
  user-select: none;
}

.base {
  transform-origin: bottom center;
  transition: transform 200ms cubic-bezier(0.22, 0.61, 0.36, 1);
}

.breathing {
  animation: ec-breathe 2.4s ease-in-out infinite;
}

.blink {
  opacity: 0;
  animation: ec-blink 3.2s infinite;
}

@keyframes ec-breathe {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(1.04);
  }
}

@keyframes ec-blink {
  0%,
  93%,
  100% {
    opacity: 0;
  }
  96% {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .breathing,
  .blink {
    animation: none;
  }
  .blink {
    opacity: 0;
  }
}
</style>
