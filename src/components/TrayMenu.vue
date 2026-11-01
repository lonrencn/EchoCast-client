<script setup lang="ts">
/**
 * 托盘菜单（自绘窗口）。
 *
 * 原生托盘菜单是系统绘制的，样式与应用主题无关；这里用一个小窗口
 * 按气泡的视觉语言画菜单：玻璃卡、accent 辉光、胶囊开关。
 * Rust 在托盘点击时把窗口摆到图标上方并 show + focus；
 * 本组件负责失焦/Esc 自动收起、菜单项动作。
 */
import { computed, onMounted, ref } from "vue";
import {
  connText,
  connTone,
  loadSettings,
  settings,
  soundOn,
} from "../settings/useSettings";
import { windowKind } from "../boot/windowKind";

/** 连接状态 → 本卡的 accent 色，与气泡四态同一套色板。 */
const TONE: Record<string, string> = {
  on: "#2FD9A4",
  wait: "#4DA6FF",
  bad: "#FF4D5E",
  off: "#8B93A7",
};

const accent = computed(() => TONE[connTone.value] ?? TONE.off);

/** 菜单里不需要完整 URL，去掉协议头就够了。 */
const serverShort = computed(() => settings.value.server_url.replace(/^wss?:\/\//, ""));

const busy = ref(false);

async function toggleSound() {
  if (busy.value) return;
  busy.value = true;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    // Rust 翻状态、落盘、广播；返回值是最新状态，直接落本地
    settings.value.sound_enabled = await invoke<boolean>("tray_toggle_sound");
  } finally {
    busy.value = false;
  }
}

async function openSettings() {
  const { invoke } = await import("@tauri-apps/api/core");
  // show_main_settings 内部会隐藏本窗口
  await invoke("show_main_settings");
}

async function openHistory() {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("show_main_history");
}

async function quit() {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("quit_app");
}

function hide() {
  void import("@tauri-apps/api/core").then(({ invoke }) =>
    invoke("tray_menu_hide")
  );
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") hide();
}

onMounted(async () => {
  // 只需要设置与连接状态（loadSettings 里已订阅 ec:conn / ec:settings-updated），
  // 不 startFeed —— 菜单窗口不该参与通知流，否则提示音会在菜单里再响一遍。
  if (windowKind === "traymenu") await loadSettings();
  window.addEventListener("keydown", onKey);
  // 点到菜单外面 = 失焦。原生菜单的行为就是这样，这里保持一致。
  window.addEventListener("blur", hide);
});
</script>

<template>
  <div class="menu" :style="{ '--accent': accent }">
    <span class="sheen" aria-hidden="true" />

    <!-- 状态头：徽标 + 连接情况，点开进设置看详情 -->
    <button class="head" @click="openSettings">
      <span class="badge" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="12" height="12">
          <path
            d="M12 2.6 4.2 7v10L12 21.4 19.8 17V7L12 2.6Zm0 5.1a2.4 2.4 0 1 1 0 4.8 2.4 2.4 0 0 1 0-4.8Z"
            fill="currentColor"
          />
        </svg>
      </span>
      <span class="head-text">
        <b>EchoCast</b>
        <em>
          <i class="dot" />
          {{ connText }}<template v-if="settings.server_url"> · {{ serverShort }}</template>
        </em>
      </span>
    </button>

    <div class="sep" aria-hidden="true" />

    <!-- 提示音：整行可点，右侧胶囊显示当前态 -->
    <button class="item" :class="{ active: soundOn }" @click="toggleSound">
      <svg class="ic" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path
          d="M8 2.5 4.8 5.2H2.5v5.6h2.3L8 13.5V2.5Z"
          fill="currentColor"
        />
        <path
          v-if="soundOn"
          d="M10.6 5.4a3.4 3.4 0 0 1 0 5.2M12.4 3.6a6 6 0 0 1 0 8.8"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
        />
        <path
          v-else
          d="M10.8 6.2l3.6 3.6M14.4 6.2l-3.6 3.6"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
        />
      </svg>
      <span class="label">提示音</span>
      <span class="pill" :class="{ on: soundOn }">{{ soundOn ? "开" : "已静音" }}</span>
    </button>

    <button class="item" @click="openHistory">
      <svg class="ic" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path
          d="M8 4.6V8l2.4 1.6"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      <span class="label">消息回看</span>
      <svg class="go" viewBox="0 0 14 14" width="11" height="11" aria-hidden="true">
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

    <button class="item" @click="openSettings">
      <svg class="ic" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path
          d="M8 1.8v1.8M8 12.4v1.8M1.8 8h1.8M12.4 8h1.8M3.6 3.6l1.3 1.3M11.1 11.1l1.3 1.3M12.4 3.6l-1.3 1.3M4.9 11.1l-1.3 1.3"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
        />
      </svg>
      <span class="label">设置…</span>
      <svg class="go" viewBox="0 0 14 14" width="11" height="11" aria-hidden="true">
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

    <div class="sep" aria-hidden="true" />

    <button class="item danger" @click="quit">
      <svg class="ic" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path
          d="M8 2v5.4"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
        />
        <path
          d="M4.2 4.4a5.4 5.4 0 1 0 7.6 0"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
        />
      </svg>
      <span class="label">退出 EchoCast</span>
    </button>
  </div>
</template>

<style scoped>
/* 气泡同款玻璃卡：accent 辉光从左上吃进暗背景，inset 描边勾出轮廓 */
.menu {
  width: 264px;
  padding: 8px;
  border-radius: 18px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 2px;
  position: relative;
  overflow: hidden;
  background:
    radial-gradient(130% 150% at 0% 8%, color-mix(in srgb, var(--accent) 20%, transparent) 0%, transparent 56%),
    linear-gradient(158deg, rgba(24, 37, 59, 0.88) 0%, rgba(11, 18, 34, 0.92) 52%, rgba(7, 12, 22, 0.96) 100%);
  -webkit-backdrop-filter: blur(16px) saturate(1.15);
  backdrop-filter: blur(16px) saturate(1.15);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.1),
    0 18px 46px -18px color-mix(in srgb, var(--accent) 46%, transparent),
    0 4px 12px rgba(0, 0, 0, 0.55);
}

/* 斜切高光：气泡卡上的玻璃反光，这里同款 */
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

/* 状态头：与气泡 topline 同构（徽标 + 主文字 + 副文字） */
.head {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 8px 9px;
  font: inherit;
  text-align: left;
  border: 0;
  border-radius: 12px;
  background: transparent;
  cursor: pointer;
  transition: background 140ms ease;
}

.head:hover {
  background: rgba(255, 255, 255, 0.055);
}

.badge {
  width: 26px;
  height: 26px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: 8px;
  color: #06121d;
  background: color-mix(in srgb, var(--accent) 88%, #ffffff);
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 55%, transparent);
}

.head-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.head-text b {
  font-size: 12.5px;
  font-weight: 600;
  letter-spacing: 0.03em;
  color: #f2f7ff;
}

.head-text em {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 10.5px;
  font-style: normal;
  color: #8494ae;
  white-space: nowrap;
  overflow: hidden;
}

.dot {
  width: 6px;
  height: 6px;
  flex: none;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 8px var(--accent);
}

.sep {
  height: 1px;
  margin: 4px 8px;
  background: rgba(255, 255, 255, 0.07);
}

.item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 9px;
  font: inherit;
  text-align: left;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: #c6d2e6;
  cursor: pointer;
  transition: background 140ms ease, color 140ms ease;
}

.item:hover {
  background: rgba(255, 255, 255, 0.055);
  color: #eef4ff;
}

.ic {
  flex: 0 0 auto;
  color: color-mix(in srgb, var(--accent) 75%, #8fa4c4);
  filter: drop-shadow(0 0 5px color-mix(in srgb, var(--accent) 60%, transparent));
}

.label {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  letter-spacing: 0.01em;
}

.go {
  flex: 0 0 auto;
  color: #5d6c85;
}

/* 开关胶囊：开 = accent 实底，静音 = 暖色提示（静音是状态不是故障） */
.pill {
  flex: 0 0 auto;
  padding: 2px 9px;
  border-radius: 999px;
  font-size: 10px;
  letter-spacing: 0.04em;
  color: #ffb3a8;
  background: rgba(255, 138, 118, 0.1);
  border: 1px solid rgba(255, 138, 118, 0.3);
}

.pill.on {
  color: #06121d;
  background: color-mix(in srgb, var(--accent) 88%, #ffffff);
  border-color: transparent;
  box-shadow: 0 0 10px color-mix(in srgb, var(--accent) 50%, transparent);
}

/* 退出是破坏性操作：给 fail 色的图标与文字，悬停加重 */
.item.danger {
  color: #f2949c;
}

.item.danger .ic {
  color: #ff4d5e;
  filter: drop-shadow(0 0 5px rgba(255, 77, 94, 0.5));
}

.item.danger:hover {
  background: rgba(255, 77, 94, 0.09);
  color: #ffb9bf;
}
</style>
