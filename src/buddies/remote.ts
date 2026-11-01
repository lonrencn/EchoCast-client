import { makeUserPack } from "./packs";
import type { BuddyManifest, BuddyPack } from "./types";

/** Rust 侧返回的用户包描述，字段与 models.rs 的 BuddyPackInfo 对应。 */
interface BuddyPackInfo {
  id: string;
  source: string;
  manifest: BuddyManifest;
  dir?: string;
  valid: boolean;
  missing: string[];
}

/** 浏览器里跑（vite dev 直接开网页）时没有 Tauri 注入，不要去 invoke。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 加载用户导入的素材包。
 *
 * 内置包走前端打包路径（构建期就被 glob 收进 dist），用户包在 appData 里，
 * 只能用 Tauri 的 asset 协议读 —— 这就是两条加载路径必须分开的原因。
 */
export async function loadUserPacks(): Promise<BuddyPack[]> {
  if (!isTauri()) return [];

  try {
    const { invoke, convertFileSrc } = await import("@tauri-apps/api/core");
    const infos = await invoke<BuddyPackInfo[]>("buddy_user_packs");

    return infos
      .filter((i) => i.valid && i.dir)
      .map((i) => makeUserPack(i.id, i.manifest, convertFileSrc(i.dir!)));
  } catch (err) {
    // 用户包读失败不能影响内置包的正常显示
    console.warn("[buddies] 用户包加载失败：", err);
    return [];
  }
}

/** 拿到用户素材目录，用于「打开文件夹」按钮。目录不存在时会在 Rust 侧创建。 */
export async function userPackDir(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string | null>("buddy_user_dir");
  } catch {
    return null;
  }
}
