/**
 * 把一条「结果」真正打开。
 *
 * 这就是「查看结果点不开」的根因：动作按钮的 payload 里带的是一个 URL
 * （服务端契约 `{label:"查看结果", url:"https://..."}`），但点击只做了
 * 一件事 —— 往服务端回一帧 action，然后气泡消失。用户什么也没看到。
 *
 * 所以这里必须真的把东西打开：桌面端走 opener 插件交给系统默认程序，
 * 浏览器预览退化成新标签页。两者都失败时返回 false，由调用方决定
 * 是提示「打不开」还是退回「把文本展开给用户看」。
 */

/** 只认这几种协议。`file:` 之类一律不放行 —— 服务端给什么就开什么太危险。 */
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

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 打开链接（或本机路径）。返回是否成功发起。
 *
 * 注意这里**不 await 用户看到**什么，只保证「交给了系统」：
 * 浏览器给的是 window.open 的返回值，Tauri 给的是插件有没有抛错。
 */
export async function openExternal(target: string): Promise<boolean> {
  const s = target.trim();
  if (!s) return false;

  if (inTauri()) {
    // 只放行 URL。本机路径不交给 opener：
    // 一是 ACL 里 `opener:default` 只允许 http/https/mailto/tel，
    // 二是「服务端给什么就开什么」等于把本机任意路径变成远程可执行入口
    // （一条 `C:////Windows////System32////calc.exe` 就够了）。路径只做复制。
    if (!isOpenableUrl(s)) return false;
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(s);
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
