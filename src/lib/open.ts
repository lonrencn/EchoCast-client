/**
 * 把一条「结果」真正打开。
 *
 * 两次返工的教训都在这：
 * 1. 动作按钮只回传帧不打开 —— 用户点了什么都没发生。必须真的交给系统。
 * 2. plugin-opener 走前端 ACL，配错 capability 时失败是**静默**的，
 *    用户看到的还是「点了没反应」。所以桌面端不再直接调插件，
 *    统一走 Rust 侧 `sys_open` 命令：ACL 管不着，白名单收在 Rust 里
 *    （http/https/mailto + 非可执行的本机路径），失败时 Err 带人话回来。
 */

/** 只认这几种协议。`file:` / `javascript:` 之类一律不放行。 */
const SAFE_SCHEMES = /^(https?|mailto):/i;

export function isOpenableUrl(raw: string | undefined | null): boolean {
  if (!raw) return false;
  const s = raw.trim();
  if (!s) return false;
  return SAFE_SCHEMES.test(s);
}

/** 本机路径：`C:\...`、`/home/...`、`\\server\share`。 */
export function looksLikePath(raw: string | undefined | null): boolean {
  if (!raw) return false;
  const s = raw.trim();
  return /^[a-zA-Z]:[\\/]/.test(s) || s.startsWith("/") || s.startsWith("\\\\");
}

/**
 * 一个目标能不能交给系统打开 —— URL 或本机绝对路径都算。
 * 这是气泡点击、动作按钮、robot 共用的判断。
 */
export function isOpenableTarget(raw: string | undefined | null): boolean {
  return isOpenableUrl(raw) || looksLikePath(raw);
}

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 打开链接（或本机路径）。返回是否成功发起。
 *
 * 桌面端统一 invoke `sys_open`：Rust 里再校验一遍协议与扩展名，
 * 失败的 reason 是写给用户看的中文，调用方直接可以摆到界面上。
 * 浏览器预览退化成新标签页。
 */
export async function openExternal(target: string): Promise<boolean> {
  const s = target.trim();
  if (!s) return false;

  if (inTauri()) {
    // URL 与本机路径都交给 Rust 侧裁决（白名单在 sys_open 里）。
    if (!isOpenableTarget(s)) return false;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("sys_open", { target: s });
      return true;
    } catch (err) {
      console.warn("[open] 打不开", s, err);
      return false;
    }
  }

  if (!isOpenableUrl(s)) return false;
  return Boolean(window.open(s, "_blank", "noopener,noreferrer"));
}

/** 复制到剪贴板。气泡上的「复制路径」和面板里的复制都走它。 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
