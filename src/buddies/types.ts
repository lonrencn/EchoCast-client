/**
 * 素材包契约。
 *
 * 核心约束：组件永远只说「我要 success 这个动作」，不知道文件叫什么、在哪、
 * 是不是 webp。文件命名、缺帧回退、内置还是用户导入，全部收敛在 pack 层。
 * 换一整套形象 = 换一个 packId，不改任何组件代码。
 */

/** 动作标识。必填三项缺一不可，其余可缺，缺了走回退链。 */
export type BuddyAction =
  | "idle"
  | "idle_blink"
  | "hover"
  | "running"
  | "success"
  | "fail"
  | "sleep";

/** 缺了这三个动作的包直接判为不合格 —— 否则会出现「失败配笑脸」。 */
export const REQUIRED_ACTIONS: readonly BuddyAction[] = ["idle", "success", "fail"];

/** 部分动作缺失时的降级顺序。最后一定落到 idle。 */
export const FALLBACK_CHAIN: Record<BuddyAction, BuddyAction[]> = {
  idle: ["idle"],
  idle_blink: ["idle_blink", "idle"],
  hover: ["hover", "idle"],
  running: ["running", "idle"],
  success: ["success", "idle"],
  fail: ["fail", "idle"],
  sleep: ["sleep", "idle"],
};

export interface BuddyActionMeta {
  file: string;
  /** 雪碧图帧数。1 表示单帧静态。 */
  frames: number;
  /** 一个完整动作的时长，毫秒。 */
  duration: number;
  loop: boolean;
  /** 非循环动作播完停在哪一帧。 */
  hold?: "last";
}

export interface BuddyManifest {
  pack: string;
  version: number;
  /** 出图边长，像素。 */
  frame: number;
  /** 建议显示尺寸，像素。 */
  display: number;
  /** 脚底基线 y。所有帧共用，保证播放时不上下跳。 */
  baseline: number;
  /** 标记为主推形象。多个包时它排在最前并作为默认。 */
  primary?: boolean;
  actions: Record<string, BuddyActionMeta>;
}

export type BuddyPackSource = "builtin" | "user";

export interface BuddyPack {
  id: string;
  source: BuddyPackSource;
  manifest: BuddyManifest;
  /** 取某动作的建议显示尺寸；包没写就用全局默认。 */
  displaySize(): number;
  /** 取某动作的最终 URL；缺失动作按回退链解析。 */
  url(action: BuddyAction): string;
  /** 某动作是否真的存在于包里（用于判断能否播放差异动作）。 */
  has(action: BuddyAction): boolean;
}

/** 包校验结果。不合格的包不进入可选列表，而不是带着缺陷硬渲染。 */
export interface PackValidation {
  ok: boolean;
  missing: BuddyAction[];
  reason?: string;
}
