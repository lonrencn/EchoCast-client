import {
  FALLBACK_CHAIN,
  REQUIRED_ACTIONS,
  type BuddyAction,
  type BuddyManifest,
  type BuddyPack,
  type BuddyPackSource,
  type PackValidation,
} from "./types";

/**
 * 内置包在构建期自动发现：往 src/buddies/builtin/ 丢一个新文件夹，
 * 里面放 manifest.json 和各动作 webp，就会自动出现在可选列表里。
 * 不需要改这个文件，也不需要手动登记。
 */
const BUILTIN_MANIFESTS = import.meta.glob("./builtin/*/manifest.json", {
  eager: true,
  import: "default",
}) as Record<string, BuddyManifest>;

const BUILTIN_ASSETS = import.meta.glob("./builtin/*/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

function packIdFromManifestPath(path: string): string {
  // "./builtin/robot/manifest.json" -> "robot"
  return path.split("/")[2];
}

/** 校验包的完整性。不合格的包绝不进入可选列表。 */
export function validateManifest(m: BuddyManifest): PackValidation {
  if (!m || typeof m !== "object") {
    return { ok: false, missing: [], reason: "manifest 解析失败" };
  }
  const actions = m.actions ?? {};
  const missing = REQUIRED_ACTIONS.filter((a) => !actions[a]?.file);
  if (missing.length) {
    return {
      ok: false,
      missing,
      reason: `缺少必需动作：${missing.join(", ")}。缺帧会导致状态与表情错配。`,
    };
  }
  if (!m.frame || m.frame < 32) {
    return { ok: false, missing: [], reason: `frame 尺寸异常：${m.frame}` };
  }
  return { ok: true, missing: [] };
}

/**
 * 沿回退链找第一个真实存在的动作。
 * 例如包里有 idle 但没有 hover，悬停就退回 idle，而不是显示裂图。
 */
function resolveAction(id: string, action: BuddyAction, actions: Record<string, unknown>): BuddyAction | null {
  for (const candidate of FALLBACK_CHAIN[action] ?? [action]) {
    if (actions[candidate]) return candidate;
  }
  void id;
  return null;
}

export function makeBuiltinPack(id: string, manifest: BuddyManifest): BuddyPack {
  return {
    id,
    source: "builtin",
    manifest,
    displaySize: () => manifest.display || 96,
    has: (action) => Boolean(manifest.actions?.[action]?.file),
    url(action) {
      const resolved = resolveAction(id, action, manifest.actions ?? {}) ?? "idle";
      const meta = manifest.actions?.[resolved];
      if (!meta) return "";
      return BUILTIN_ASSETS[`./builtin/${id}/${meta.file}`] ?? "";
    },
  };
}

/** 用户导入的包：manifest 由 Rust 侧读取后传进来，图片走 Tauri 资产协议。 */
export function makeUserPack(
  id: string,
  manifest: BuddyManifest,
  dirUrl: string
): BuddyPack {
  const base = dirUrl.endsWith("/") ? dirUrl : `${dirUrl}/`;
  return {
    id,
    source: "user",
    manifest,
    displaySize: () => manifest.display || 96,
    has: (action) => Boolean(manifest.actions?.[action]?.file),
    url(action) {
      const resolved = resolveAction(id, action, manifest.actions ?? {}) ?? "idle";
      const meta = manifest.actions?.[resolved];
      return meta ? `${base}${meta.file}` : "";
    },
  };
}

let builtinCache: BuddyPack[] | null = null;

/** 列出所有可用的内置包。不合规的会被剔除并给出原因。 */
export function listBuiltinPacks(): BuddyPack[] {
  if (builtinCache) return builtinCache;

  const packs: BuddyPack[] = [];
  for (const [path, manifest] of Object.entries(BUILTIN_MANIFESTS)) {
    const id = packIdFromManifestPath(path);
    const check = validateManifest(manifest);
    if (!check.ok) {
      console.warn(`[buddies] 跳过不合格的包 ${id}: ${check.reason}`);
      continue;
    }
    packs.push(makeBuiltinPack(id, manifest));
  }
  // 主推包优先，其余按字母序：默认展示的必须是完整的那套
  const rank = (pack: BuddyPack) => (pack.manifest.primary ? 0 : 1);
  packs.sort((a, b) => rank(a) - rank(b) || a.id.localeCompare(b.id));
  builtinCache = packs;
  return packs;
}

/** 按 id 找包；找不到就落到第一个可用包，保证组件永远能渲染出东西。 */
export function getBuiltinPack(id?: string): BuddyPack | null {
  const all = listBuiltinPacks();
  if (!all.length) return null;
  return all.find((p) => p.id === id) ?? all[0];
}

/**
 * 预解码。雪碧图和静态 webp 都是 <img> 加载，
 * 不做预解码的话，第一次切状态会出现一帧空白。
 */
export function preloadPack(pack: BuddyPack, actions: BuddyAction[] = ["idle"]): Promise<void> {
  const urls = Array.from(new Set(actions.map((a) => pack.url(a)).filter(Boolean)));
  return Promise.all(
    urls.map(
      (u) =>
        new Promise<void>((resolve) => {
          const img = new Image();
          img.onload = () => resolve();
          img.onerror = () => resolve();
          img.src = u;
        })
    )
  ).then(() => undefined);
}

export type { BuddyPack, BuddyPackSource };
