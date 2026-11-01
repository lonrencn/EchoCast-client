<script setup lang="ts">
import { computed } from "vue";
import { MUTE_HOTKEY, soundOn, toggleSound } from "../settings/useSettings";

/**
 * 一键静音。
 *
 * 为什么它必须能出现在「通知以外」的地方：设置面板藏在齿轮后面，
 * 而用户要静音的时刻，恰恰是通知正在一条接一条响、他不想再看见任何弹窗的时刻。
 * 所以这个按钮要能直接摆在标题栏和灯条上，按一下就哑，不问第二句。
 *
 * 文案只说结果（「静音」/「取消静音」），不解释机制 —— 机制在提示里。
 */
withDefaults(
  defineProps<{
    /** sm = 灯条/工具栏里的 22px；md = 页面标题栏。 */
    size?: "sm" | "md";
  }>(),
  { size: "sm" }
);

/** 只看「会不会出声」：音量拖到 0 和关掉开关，对用户是同一种结果。 */
const muted = computed(() => !soundOn.value);

const label = computed(() => (muted.value ? `取消静音（${MUTE_HOTKEY}）` : `静音（${MUTE_HOTKEY}）`));
</script>

<template>
  <button
    class="mute"
    :class="[`s-${size}`, { muted }]"
    :aria-pressed="muted"
    :aria-label="label"
    :title="label"
    type="button"
    @click.stop="toggleSound()"
  >
    <!-- 喇叭本体 + 两道声波；静音时声波换成一道斜杠，不靠颜色单打独斗 -->
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path
        class="cone"
        d="M3.6 9.4h3.1L11.4 5.4v13.2L6.7 14.6H3.6z"
      />
      <template v-if="!muted">
        <path class="wave" d="M14.9 9.2a4.2 4.2 0 0 1 0 5.6" />
        <path class="wave far" d="M17.5 6.7a7.8 7.8 0 0 1 0 10.6" />
      </template>
      <path v-else class="slash" d="M15 9l5 6M20 9l-5 6" />
    </svg>
  </button>
</template>

<style scoped>
.mute {
  flex: none;
  display: grid;
  place-items: center;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: #7c8aa2;
  cursor: pointer;
  transition:
    background 140ms ease,
    color 140ms ease;
}

.s-sm {
  width: 22px;
  height: 22px;
}

.s-md {
  width: 24px;
  height: 24px;
}

.mute svg {
  width: 14px;
  height: 14px;
  overflow: visible;
}

.s-md svg {
  width: 15px;
  height: 15px;
}

.cone {
  fill: currentColor;
}

.wave,
.slash {
  fill: none;
  stroke: currentColor;
  stroke-width: 1.7;
  stroke-linecap: round;
}

.wave.far {
  opacity: 0.55;
}

.mute:hover {
  color: #eaf2ff;
  background: rgba(255, 255, 255, 0.08);
}

/* 静音不是「坏」，但必须比「开着」显眼一点 —— 否则用户忘了自己关过，
   下次抱怨「怎么没声音」。用暖红而不是报错红，避免看着像故障。 */
.mute.muted {
  color: #ffb3b8;
  background: rgba(255, 77, 94, 0.1);
}

.mute.muted:hover {
  color: #ffd0d4;
  background: rgba(255, 77, 94, 0.18);
}

.mute:focus-visible {
  outline: 2px solid rgba(127, 232, 255, 0.6);
  outline-offset: 1px;
}
</style>
