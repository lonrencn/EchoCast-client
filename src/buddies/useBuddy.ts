import { computed, ref, watch } from "vue";
import { saveSettings, settings } from "../settings/useSettings";
import { listBuiltinPacks, preloadPack } from "./packs";
import { loadUserPacks } from "./remote";
import type { BuddyAction, BuddyPack } from "./types";

/** 全局当前选中的包，设置面板改这里，所有气泡跟着变。 */
const activePackId = ref<string | undefined>(undefined);

/**
 * 设置加载完成后恢复上次选的那套。
 * 空串表示用户从没选过，此时保持 undefined，让 packs 排序里的 primary 生效。
 */
watch(
  () => settings.value.buddy_pack,
  (id) => {
    if (id) activePackId.value = id;
  },
  { immediate: true }
);

/** 用户导入的包。异步加载，加载完成前不影响内置包显示。 */
const userPacks = ref<BuddyPack[]>([]);
let userLoadStarted = false;

function ensureUserPacks() {
  if (userLoadStarted) return;
  userLoadStarted = true;
  void loadUserPacks().then((list) => {
    userPacks.value = list;
  });
}

/** 预加载过的包，避免重复解码。 */
const warmed = new Set<string>();

const PREFETCH: BuddyAction[] = ["idle", "success", "fail", "running", "excited"];

/**
 * 组件侧唯一入口。组件只声明「我要哪个包 + 哪个状态」，
 * 具体文件、回退、URL 全部由 pack 层决定。
 */
export function useBuddy(packId?: () => string | undefined) {
  ensureUserPacks();

  const builtin = listBuiltinPacks();
  const packs = computed<BuddyPack[]>(() => [...builtin, ...userPacks.value]);

  const pack = computed<BuddyPack | null>(() => {
    const wanted = packId?.() ?? activePackId.value;
    const all = packs.value;
    if (wanted) {
      const hit = all.find((p) => p.id === wanted);
      if (hit) return hit;
    }
    // 指定的包不存在（被删了 / 名字打错）就落到排第一的那个，
    // 保证组件永远能渲染出东西，而不是白屏。
    return all[0] ?? null;
  });

  watch(
    pack,
    (p) => {
      if (!p || warmed.has(p.id)) return;
      warmed.add(p.id);
      void preloadPack(p, PREFETCH);
    },
    { immediate: true }
  );

  /** 取某状态的图片地址。空串表示这个包连 idle 都没有，调用方应回退到纯文字。 */
  function src(action: BuddyAction): string {
    return pack.value?.url(action) ?? "";
  }

  /** 这个包是否真的带该动作；用来决定要不要播额外动画（眨眼、悬停抬头）。 */
  function supports(action: BuddyAction): boolean {
    return pack.value?.has(action) ?? false;
  }

  return {
    pack,
    packs,
    activePackId,
    /**
     * 换包 = 换一整套形象。同时落盘，下次启动还是这一套 ——
     * 选完重开就变回默认，用户会以为设置没生效。
     */
    setPack: (id: string) => {
      activePackId.value = id;
      void saveSettings({ buddy_pack: id });
    },
    src,
    supports,
  };
}
