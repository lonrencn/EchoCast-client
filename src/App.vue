<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import NoticeStack from "./components/NoticeStack.vue";
import TopBarStrip from "./components/TopBarStrip.vue";
import NoticeSheet from "./components/NoticeSheet.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import HistoryPanel from "./components/HistoryPanel.vue";
import TrayMenu from "./components/TrayMenu.vue";
import { openSheet, runAction, startFeed } from "./feed/notices";
import { applyOverlay, loadSettings, toggleSound } from "./settings/useSettings";
import { isSurface, windowKind } from "./boot/windowKind";

/** 主窗口两块视图：设置 / 消息回看。托盘菜单按需把人带到其中一块。 */
const mainView = ref<"settings" | "history">("settings");

/** 设置要先把上次的地址和形象包读进来，再判断走哪条数据源。 */
onMounted(async () => {
  await loadSettings();

  // 托盘菜单窗口：只吃设置与连接状态（loadSettings 里已订阅），
  // 不进通知流 —— 不然提示音会在菜单里再响一遍。
  if (windowKind === "traymenu") return;

  void startFeed();

  // 主窗口听托盘的调度：回看 / 设置，切到对应视图。
  if (windowKind === "main") {
    await applyOverlay();
    const { listen } = await import("@tauri-apps/api/event");
    await listen("ec:show-history", () => (mainView.value = "history"));
    await listen("ec:open-settings", () => (mainView.value = "settings"));
  }
});

/**
 * 全局静音快捷键。
 *
 * 挂 window 而不是绑在某个浮层上：设置窗口没聚焦、灯条收起、气泡全散的时候，
 * 用户仍然要能一键把声音掐掉 —— 响着的提示音等不了先找到按钮。
 */
function onHotkey(e: KeyboardEvent) {
  if (windowKind === "traymenu") return; // 菜单窗口只认 Esc
  if (!e.ctrlKey || !e.shiftKey || e.altKey) return;
  if (e.key.toLowerCase() !== "m") return;
  e.preventDefault();
  void toggleSound();
}

onMounted(() => window.addEventListener("keydown", onHotkey));
onBeforeUnmount(() => window.removeEventListener("keydown", onHotkey));

/** 悬浮层上的齿轮没有设置面板可开，把请求转回主窗口。 */
async function emitOpenSettings() {
  const { emitTo } = await import("@tauri-apps/api/event");
  await emitTo("main", "ec:open-settings");
}
</script>

<template>
  <!-- 托盘菜单窗口：只渲染菜单本身 -->
  <TrayMenu v-if="windowKind === 'traymenu'" />

  <!-- 悬浮窗口（右下气泡 / 顶部灯条）：只渲染通知面，没有页面 chrome -->
  <template v-else-if="isSurface">
    <NoticeStack v-if="windowKind === 'overlay'" />
    <TopBarStrip
      v-else
      position="fixed"
      @settings="emitOpenSettings"
      @action="runAction"
      @sheet="openSheet"
    />
    <!-- 问询的作答面板跟着气泡走：用户在悬浮层上点「等你答复…」，
         面板必须就近弹出，而不是跑到主窗口里去 -->
    <NoticeSheet />
  </template>

  <!-- 主窗口 = 设置 / 消息回看。通知走桌面悬浮层，这里不再演示任何东西 -->
  <main v-else class="home">
    <HistoryPanel
      v-if="mainView === 'history'"
      @back="mainView = 'settings'"
    />
    <SettingsPanel v-else embedded />
    <NoticeSheet />
  </main>
</template>

<style>
:root {
  font-family: "Inter", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;
  color: #e8eef9;
  background: #070b14;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  background:
    radial-gradient(120% 90% at 18% 0%, #14213a 0%, transparent 55%),
    radial-gradient(100% 80% at 90% 30%, #1a1030 0%, transparent 50%),
    #070b14;
  background-attachment: fixed;
}

/* 悬浮窗口（气泡层 / 灯条）：整体透明，通知之外的区域一个像素都不能画。
   :root 上那层 #070b14 必须覆盖掉，否则桌面上会糊出一块深色底板。 */
html.overlay-surface,
html.overlay-surface body {
  background: transparent;
}

/* 设置窗口：面板铺满，页面不再提供任何布局。
   display:contents —— main 不产生盒子，否则空壳 100vh + 面板 100vh
   会让页面变成 200vh，窗口右侧多出一条没人要的滚动条。 */
.home {
  display: contents;
}
</style>
