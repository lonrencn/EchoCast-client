/**
 * 这个 JS 上下文跑在哪个窗口里。
 *
 * 同一份 index.html 会被三个窗口加载：主控制台、右下气泡层、顶部灯条。
 * 各窗口只渲染自己该渲染的东西 —— 全都渲染的话，气泡会同时出现在
 * 三个位置，提示音也会响三遍。
 *
 * 悬浮窗口由 Rust 侧 initialization_script 注入 `__ECHOCAST_LABEL__`，
 * 主窗口没有注入量，走 Tauri 环境判断兜底。
 */

export type WindowKind = "main" | "overlay" | "topbar" | "traymenu" | "browser";

declare global {
  interface Window {
    __ECHOCAST_LABEL__?: string;
  }
}

const injected = window.__ECHOCAST_LABEL__;

export const windowKind: WindowKind =
  injected === "overlay" || injected === "topbar" || injected === "traymenu"
    ? injected
    : "__TAURI_INTERNALS__" in window
      ? "main"
      : "browser";

/** 这个窗口是不是「真正显示通知的面」。 */
export const isSurface = windowKind === "overlay" || windowKind === "topbar";

/** 托盘菜单窗口：和悬浮层一样整窗透明，但不消费通知流。 */
if (isSurface || windowKind === "traymenu") {
  // 悬浮窗口必须全透明：背景色哪怕 1px 都会在桌面上糊出一块色斑
  document.documentElement.classList.add("overlay-surface");
}
