<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useBuddy } from "../buddies/useBuddy";
import MuteButton from "./MuteButton.vue";
import { previewSfx } from "../audio/sfx";
import type { TaskStatus } from "../types/notice";
import {
  busy,
  configPath,
  conn,
  connFatal,
  connText,
  connTone,
  connectServer,
  DEFAULT_TOPICS,
  desktop,
  disconnectServer,
  displayMode,
  isConnected,
  localDemo,
  MUTE_HOTKEY,
  removeEndpoint,
  saveCurrentAsEndpoint,
  saveSettings,
  setDisplayMode,
  settings,
  settingsError,
  soundOn,
  startLocalDemo,
  switchEndpointNow,
  terminal,
  toggleSound,
  type EndpointProfile,
} from "../settings/useSettings";

const open = defineModel<boolean>("open", { default: false });

/**
 * 嵌入模式：主窗口现在就是设置窗口，面板直接铺满窗口，
 * 没有遮罩、没有弹出动画、没有关闭按钮。独立抽屉模式保留给浏览器预览。
 */
const props = defineProps<{ embedded?: boolean }>();
const isOpen = computed(() => props.embedded || open.value);

/**
 * 面板 accent 色 = 连接状态色，与气泡四态同一套色板。
 * 卡片辉光、描边、主按钮全部从这里派生 —— 状态一眼可读。
 */
const TONE: Record<string, string> = {
  on: "#2FD9A4",
  wait: "#4DA6FF",
  bad: "#FF4D5E",
  off: "#8B93A7",
};
const accent = computed(() => TONE[connTone.value] ?? TONE.off);

/** 试听按钮。顺序按任务生命周期排，不要按字母排。 */
const SFX_PREVIEW: { status: TaskStatus; label: string; icon: string }[] = [
  { status: "running", label: "开始", icon: "▷" },
  { status: "success", label: "完成", icon: "✓" },
  { status: "warning", label: "警告", icon: "!" },
  { status: "failed", label: "失败", icon: "×" },
];

const { packs, activePackId, setPack } = useBuddy();

/** 输入框里的是草稿，点「连接」才落到设置里 —— 边打字边重连会把服务端打爆。 */
const draft = ref(settings.value.server_url);
const copied = ref(false);
/** token 和 topic 是连上之前就要用的，改完必须重新连接才生效。 */
const tokenDraft = ref(settings.value.token);
const topicsDraft = ref(settings.value.topics);
/** 终端别名与标签：注册时上报给服务端，服务端据此路由。 */
const aliasDraft = ref(settings.value.terminal_alias);
const labelsDraft = ref(settings.value.terminal_labels);
/** 本机 device_id，没连上时用它说明「这台机器是谁」。 */
const deviceId = ref("");

watch(
  () => settings.value.server_url,
  (v) => {
    if (!busy.value) draft.value = v;
  }
);

watch(
  () => settings.value.token,
  (v) => (tokenDraft.value = v)
);

watch(
  () => settings.value.topics,
  (v) => (topicsDraft.value = v)
);

watch(
  () => settings.value.terminal_alias,
  (v) => (aliasDraft.value = v)
);

watch(
  () => settings.value.terminal_labels,
  (v) => (labelsDraft.value = v)
);

async function toggleAuto() {
  await saveSettings({ auto_connect: !settings.value.auto_connect });
}

/**
 * 拖动时只改本地值，松手才落盘。
 * 不落盘听不到音量变化，每移一步就写一次文件又会把 settings.json 刷爆。
 */
let volTimer: number | undefined;

function onVolume(e: Event) {
  const v = Number((e.target as HTMLInputElement).value) / 100;
  settings.value.sound_volume = v;
  window.clearTimeout(volTimer);
  volTimer = window.setTimeout(() => void saveSettings({ sound_volume: v }), 400);
}

function onVolumeCommit() {
  window.clearTimeout(volTimer);
  void saveSettings({ sound_volume: settings.value.sound_volume });
  // 松手立刻用新音量放一声，不然「调到多少才合适」只能靠猜
  previewSfx("success");
}

/** 取本机 device_id。只在桌面端有意义，失败就不显示。 */
async function loadDevice() {
  if (!desktop.value || deviceId.value) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const info = await invoke<{ device_id: string }>("device_info");
    deviceId.value = info.device_id;
  } catch {
    // 拿不到就不显示，不值得为一行展示信息报错
  }
}

watch(isOpen, (v) => {
  if (!v) return;
  syncDrafts();
});

/** 面板可能一进来就是打开的（主窗口嵌入式 / 独立窗口），
    只在 watch 里取会让「本机身份」一直显示「—」。 */
function syncDrafts() {
  draft.value = settings.value.server_url;
  tokenDraft.value = settings.value.token;
  topicsDraft.value = settings.value.topics;
  aliasDraft.value = settings.value.terminal_alias;
  labelsDraft.value = settings.value.terminal_labels;
  copied.value = false;
  void loadDevice();
}
if (isOpen.value) syncDrafts();

async function saveToken() {
  await saveSettings({ token: tokenDraft.value });
}

async function saveTopics() {
  await saveSettings({ topics: topicsDraft.value });
}

// ── 订阅类型：默认全订阅，收进折叠区，改过才展开 ──
const DEFAULT_TOPIC_COUNT = DEFAULT_TOPICS.split(",").filter(Boolean).length;
const topicCount = computed(
  () => topicsDraft.value.split(",").map((s) => s.trim()).filter(Boolean).length
);
const topicsCustom = computed(() => topicsDraft.value.trim() !== DEFAULT_TOPICS);

async function resetTopics() {
  topicsDraft.value = DEFAULT_TOPICS;
  await saveTopics();
}

async function saveIdentity() {
  await saveSettings({
    terminal_alias: aliasDraft.value,
    terminal_labels: labelsDraft.value,
  });
}

/** 地址预览。用户在点击前就该看到「它到底会连到哪」。 */
const preview = computed(() => {
  const s = draft.value.trim();
  if (!s) return "";
  if (/^wss?:\/\//.test(s)) return s;
  if (s.startsWith("https://")) return `wss://${s.slice(8)}`;
  if (s.startsWith("http://")) return `ws://${s.slice(7)}`;
  return `ws://${s}`;
});

const dirty = computed(() => draft.value.trim() !== settings.value.server_url);

/**
 * 主按钮一次只做一件事：
 * 有地址 → 连接/重连；地址被清空 → 变成一个显式的「清除地址」。
 * 把清除藏进主按钮，比在按钮行塞第三个 10px 小字链接可靠得多。
 */
const primaryLabel = computed(() => {
  if (!draft.value.trim()) {
    // 已经清干净了就别再显示「清除地址」—— 那会让人以为还有东西没删掉
    return settings.value.server_url ? "清除地址" : "连接";
  }
  return isConnected.value ? "重新连接" : "连接";
});

const canConnect = computed(
  () =>
    desktop.value &&
    !busy.value &&
    (draft.value.trim().length > 0 || Boolean(settings.value.server_url))
);

const mockVisible = import.meta.env.DEV;

async function submit() {
  if (!canConnect.value) return;

  // 地址空着还按下去，意思就是「我不连了」—— 顺手把存盘地址也清掉，
  // 否则下次启动还会去连一个用户已经放弃的地址。
  if (!draft.value.trim()) {
    await doClear();
    return;
  }

  const ok = await connectServer(draft.value);
  if (ok) draft.value = settings.value.server_url;
}

async function doClear() {
  await disconnectServer();
  await saveSettings({ server_url: "" });
  draft.value = "";
}

/* ── 激活码入网（无令牌新用户的第一入口）───────────────────── */
const showActivate = ref(false);
const activateCode = ref("");
const activateAlias = ref("");
const activating = ref(false);
const activateMsg = ref("");
const activateOk = ref(false);

/** 服务端自述的入网指引（可选能力；null = 服务端没提供，显示通用文案） */
const srvInfo = ref<{ name?: string; join_doc?: string; contact?: string } | null>(null);

async function loadServerInfo() {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    srvInfo.value = await invoke("server_info");
  } catch {
    srvInfo.value = null;
  }
}

async function openJoinDoc() {
  if (!srvInfo.value?.join_doc) return;
  const { openExternal } = await import("../lib/open");
  void openExternal(srvInfo.value.join_doc);
}

watch(showActivate, (on) => {
  if (on) void loadServerInfo();
});

async function doActivate() {
  const code = activateCode.value.trim();
  if (activating.value || !code) return;
  if (!draft.value.trim()) {
    activateOk.value = false;
    activateMsg.value = "先在上面填好服务端地址，再激活";
    return;
  }
  activating.value = true;
  activateMsg.value = "";
  activateOk.value = false;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const res = await invoke<{
      token: string;
      terminal_id: string;
      alias: string;
    }>("terminal_activate", {
      code,
      alias: activateAlias.value.trim() || null,
    });
    activateOk.value = true;
    activateMsg.value = `入网成功 · ${res.alias || res.terminal_id}（${res.terminal_id}），正在连接…`;
    activateCode.value = "";
    // Rust 侧已把 token 落盘；这里同步界面显示，免得密码框还是空的
    tokenDraft.value = res.token;
    settings.value.token = res.token;
    await submit();
  } catch (err) {
    activateOk.value = false;
    activateMsg.value = String(err);
  } finally {
    activating.value = false;
  }
}

async function toggleMock() {
  await saveSettings({ mock_enabled: !settings.value.mock_enabled });
}

async function pickPack(id: string) {
  // setPack 内部负责落盘，这里不再重复保存一次
  setPack(id);
}

/**
 * 打开随包资料 / 用户形象包目录。
 *
 * 路径解析全在 Rust 侧（guides.rs）：安装目录在用户机器上各不相同，
 * 前端不该猜；Rust 的 open_local_dir 只放行资料目录和用户包目录，
 * 不做成通用文件系统入口。浏览器预览没有 IPC，直接提示。
 *
 * 报错就近显示在形象包区块（forgeError），不再甩进服务端区 ——
 * 用户点的是这里的按钮，错误出现在窗口另一头只会看不懂。
 */
const forgeError = ref("");

async function openPath(
  pick: (
    invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>
  ) => Promise<string | null>
): Promise<void> {
  forgeError.value = "";
  if (!desktop.value) {
    forgeError.value = "浏览器预览没有本地目录，请在桌面端打开";
    return;
  }
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const dir = await pick(invoke);
    if (!dir) {
      forgeError.value = "资料目录不存在（可能安装包不完整）";
      return;
    }
    await invoke("open_local_dir", { path: dir });
  } catch (err) {
    forgeError.value = String(err);
  }
}

const openPacks = () =>
  openPath((invoke) => invoke<string | null>("buddy_user_dir"));
const openGuide = () =>
  openPath((invoke) => invoke<string | null>("forge_dir"));
const openApi = () =>
  openPath((invoke) => invoke<string | null>("guides_dir"));

async function copyPath() {
  if (!configPath.value) return;
  try {
    await navigator.clipboard.writeText(configPath.value);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1400);
  } catch {
    // 剪贴板被拒时至少让路径可见可选中，不做额外报错
  }
}

// ---------------------------------------------------------------------------
// 服务端点：多环境切换
// ---------------------------------------------------------------------------

const epLabel = ref("");
const epBusy = ref(false);

/** 把当前生效的地址/令牌/订阅快照成一条端点。 */
async function addEndpoint() {
  if (epBusy.value) return;
  epBusy.value = true;
  const ok = await saveCurrentAsEndpoint(epLabel.value);
  if (ok) epLabel.value = "";
  epBusy.value = false;
}

/** 点端点 = 断开旧连接，按它的三件套重连。当前生效的再点一次不动。 */
async function pickEndpoint(e: EndpointProfile) {
  if (epBusy.value || e.id === settings.value.active_endpoint) return;
  epBusy.value = true;
  await switchEndpointNow(e.id);
  epBusy.value = false;
}

// ---------------------------------------------------------------------------
// 调试：服务端原始帧。入口刻意隐蔽 —— 底栏配置路径 3 秒内连点 5 次。
// ---------------------------------------------------------------------------

interface DebugFrame {
  ts: number;
  dir: string;
  text: string;
}

const debugOpen = ref(false);
const debugFrames = ref<DebugFrame[]>([]);
let dbgTimer: number | undefined;
let taps = 0;
let tapTimer: number | undefined;

/** 第一次点击仍复制路径；连点计数达标后开关调试区。 */
function secretTap() {
  void copyPath();
  taps += 1;
  window.clearTimeout(tapTimer);
  tapTimer = window.setTimeout(() => (taps = 0), 3000);
  if (taps >= 5) {
    taps = 0;
    debugOpen.value = !debugOpen.value;
  }
}

async function loadDebug() {
  if (!desktop.value) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    debugFrames.value = await invoke<DebugFrame[]>("debug_frames");
  } catch {
    // 调试面板不弹错 —— 它自己坏了也不该打扰人
  }
}

async function clearDebug() {
  if (!desktop.value) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("debug_clear");
    debugFrames.value = [];
  } catch {
    // 同上
  }
}

async function copyDebug() {
  try {
    await navigator.clipboard.writeText(
      debugFrames.value.map((f) => `${f.ts} ${f.dir} ${f.text}`).join("\n")
    );
  } catch {
    // 剪贴板被拒就不复制，列表仍可手动选
  }
}

function dbgTime(ts: number): string {
  const d = new Date(ts);
  const p = (x: number) => String(x).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 打开期间每 2 秒拉一次最新帧；关掉即停。 */
watch(debugOpen, (v) => {
  window.clearInterval(dbgTimer);
  if (v) {
    void loadDebug();
    if (desktop.value) {
      dbgTimer = window.setInterval(() => void loadDebug(), 2000);
    }
  }
});

onBeforeUnmount(() => window.clearInterval(dbgTimer));
</script>

<template>
  <!-- 嵌入模式下不传送到 body、不套过渡和弹层样式：
       设置就是窗口内容本身，一个普通页面，不是浮在窗口里的第二张卡 -->
  <Teleport to="body" :disabled="embedded">
    <Transition name="veil">
      <div v-if="!embedded && open" class="veil" @click="open = false" />
    </Transition>

    <Transition :name="embedded ? 'none' : 'pop'">
      <aside
        v-if="isOpen"
        :class="embedded ? 'page' : 'float'"
        role="dialog"
        aria-label="设置"
        :style="{ '--accent': accent }"
      >
        <span v-if="!embedded" class="sheen" aria-hidden="true" />

        <header class="head">
          <span class="badge" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="13" height="13">
              <path
                d="M12 2.6 4.2 7v10L12 21.4 19.8 17V7L12 2.6Zm0 5.1a2.4 2.4 0 1 1 0 4.8 2.4 2.4 0 0 1 0-4.8Z"
                fill="currentColor"
              />
            </svg>
          </span>
          <div class="head-text">
            <h2>EchoCast 设置</h2>
            <p>服务端 · 形象 · 声音</p>
          </div>
          <span class="chip">
            <i class="dot" />
            {{ connText }}
          </span>
          <button v-if="!embedded" class="close" aria-label="关闭" @click="open = false">✕</button>
        </header>

        <div class="body">
          <!-- ── 服务端：地址/令牌/订阅 + 已存端点，一张卡说清一件事 ── -->
          <section>
            <div class="sec-head">
              <span class="sec-title"><i />服务端</span>
            </div>

            <label class="field">
              <input
                v-model="draft"
                class="input"
                type="text"
                spellcheck="false"
                autocomplete="off"
                placeholder="wss://服务器地址:端口/ws"
                @keyup.enter="submit"
              />
            </label>

            <p v-if="preview && preview !== draft.trim()" class="hint">
              将连接 <code>{{ preview }}</code>
            </p>
            <p v-else class="hint">
              也接受 <code>host:port</code> 和 <code>http://</code>，会自动补成 ws。
            </p>

            <label class="field">
              <input
                v-model="tokenDraft"
                class="input"
                type="password"
                spellcheck="false"
                autocomplete="off"
                placeholder="接入令牌（服务端不校验就留空）"
                @change="saveToken"
              />
            </label>

            <!-- 激活码入网：没有 token 的新用户从这里开始。
                 契约：POST /api/terminals/activate（docs/terminal-routing.md §4.1 乙）。
                 成功后令牌自动落设置，全程不手抄 hex。 -->
            <div class="act-toggle">
              <button class="btn ghost slim" type="button" @click="showActivate = !showActivate">
                {{ showActivate ? "收起" : "没有令牌？用激活码入网 →" }}
              </button>
            </div>
            <div v-if="showActivate" class="act-box">
              <!-- 服务端自述的入网指引（可选能力）：内容部署方自定义，客户端只展示。
                   拉不到就退回通用文案——不假设任何发放流程，这是两端解耦的边界。 -->
              <div v-if="srvInfo" class="srv-info">
                <span class="srv-name">{{ srvInfo.name || "该服务端" }} · 入网指引</span>
                <button
                  v-if="srvInfo.join_doc"
                  class="btn ghost slim"
                  type="button"
                  title="打开入网指引"
                  @click="openJoinDoc"
                >
                  {{ srvInfo.join_doc }} ↗
                </button>
                <span v-if="srvInfo.contact" class="srv-contact">{{ srvInfo.contact }}</span>
              </div>
              <p v-else class="hint">
                申请令牌请联系该服务端的管理员（获取激活码或直接给令牌）。
              </p>
              <label class="field">
                <input
                  v-model="activateCode"
                  class="input"
                  type="text"
                  spellcheck="false"
                  autocomplete="off"
                  placeholder="激活码（找服务端管理员申请，10 分钟有效）"
                  @keydown.enter="doActivate"
                />
              </label>
              <label class="field">
                <input
                  v-model="activateAlias"
                  class="input"
                  type="text"
                  spellcheck="false"
                  autocomplete="off"
                  placeholder="这台机器叫什么（可选，默认用计算机名）"
                />
              </label>
              <div class="row">
                <button
                  class="btn primary"
                  :disabled="activating || !activateCode.trim() || !draft.trim()"
                  @click="doActivate"
                >
                  {{ activating ? "激活中…" : "激活并连接" }}
                </button>
              </div>
              <p v-if="activateMsg" class="hint" :class="{ ok: activateOk }">
                {{ activateMsg }}
              </p>
              <p class="hint">
                一个激活码只能激活一台终端、10 分钟内有效；成功后令牌自动填入，
                不需要手抄。地址填好再激活。
              </p>
            </div>

            <!-- 订阅清单不是普通人该碰的东西：默认全订阅，收进折叠区，
                 只有真的改过才默认展开 -->
            <details class="adv" :open="topicsCustom">
              <summary>
                <span>订阅通知类型</span>
                <em>{{ topicsCustom ? `已改 · ${topicCount} 类` : `默认 ${DEFAULT_TOPIC_COUNT} 类` }}</em>
              </summary>
              <div class="adv-body">
                <label class="field">
                  <input
                    v-model="topicsDraft"
                    class="input"
                    type="text"
                    spellcheck="false"
                    autocomplete="off"
                    placeholder="topic 名称，逗号分隔"
                    @change="saveTopics"
                  />
                </label>
                <div class="row">
                  <button class="btn ghost" :disabled="!topicsCustom" @click="resetTopics">
                    恢复默认订阅
                  </button>
                </div>
                <p class="hint">服务端只推这里列出的类型。不动它就是全部订阅。</p>
              </div>
            </details>

            <p class="hint">
              token 与<b>一台终端</b>一一对应，服务端靠它认出这条连接是谁，再决定把哪些任务发过来。
              改了 token 或 topic 要重新连接。
            </p>

            <div class="row">
              <button class="btn primary" :disabled="!canConnect" @click="submit">
                {{ primaryLabel }}
              </button>
              <button
                v-if="isConnected || conn.state === 'error'"
                class="btn ghost"
                @click="disconnectServer"
              >
                断开
              </button>
              <span v-if="dirty && isConnected" class="tag">地址已改，未应用</span>
            </div>

            <p v-if="settingsError" class="err">{{ settingsError }}</p>
            <p v-else-if="connFatal" class="err">
              {{ connFatal }}<br />
              <span class="err-sub">已停止重连，改完配置点「连接」重试。</span>
            </p>
            <p v-else-if="conn.state === 'error'" class="err">{{ conn.message }}</p>
            <p v-else-if="!desktop" class="warn">
              浏览器预览里没有连接能力，用桌面端打开才能连服务端。
            </p>

            <!-- 本地演示服务端：第一次使用的引导入口。
                 一键启动、自动连接，立刻能看到一轮完整的任务通知 -->
            <div class="local-demo">
              <button
                class="btn ghost"
                :disabled="busy || localDemo?.running"
                @click="startLocalDemo"
              >
                使用本地服务端
              </button>
              <span v-if="localDemo?.running" class="local-status">
                <i />
                演示服务端运行中 · ws://127.0.0.1:{{ localDemo.port }} ·
                {{ localDemo.clients }} 台连接
              </span>
              <span v-else class="local-tip">第一次用？点它立刻看到通知效果</span>
            </div>

            <!-- 已存端点：挂在服务端卡内的从属区，添加一个计数就 +1 -->
            <div class="ep-block">
              <div class="ep-head">
                <span class="ep-title">服务端点</span>
                <span class="chip alt">已添加 {{ settings.endpoints.length }} 个</span>
              </div>

              <p class="ep-guide">
                <b>第一次用？</b>点上方「使用本地服务端」先看效果；连好正式环境后，
                在这里点「＋ 添加服务端点」把它存起来，以后一键切换。
              </p>

              <div v-if="settings.endpoints.length" class="ep-list">
                <div
                  v-for="e in settings.endpoints"
                  :key="e.id"
                  class="ep"
                  :class="{ on: settings.active_endpoint === e.id }"
                >
                  <button class="ep-main" :disabled="epBusy" @click="pickEndpoint(e)">
                    <b>{{ e.label || "(未命名)" }}</b>
                    <em>{{ e.url }}</em>
                  </button>
                  <button class="ep-del" :disabled="epBusy" title="删除" @click="removeEndpoint(e.id)">
                    ✕
                  </button>
                </div>
              </div>
              <p v-else class="hint">
                还没有添加过。连上上面的服务端后，点「＋ 添加服务端点」把这条连接存起来，以后一键切换。
              </p>

              <div class="ep-add">
                <input
                  v-model="epLabel"
                  class="input"
                  type="text"
                  spellcheck="false"
                  autocomplete="off"
                  placeholder="给端点起个名字（可选，如：公司网关）"
                  @keyup.enter="addEndpoint"
                />
                <button class="btn ghost" :disabled="epBusy || !settings.server_url" @click="addEndpoint">
                  ＋ 添加服务端点
                </button>
              </div>

              <p class="hint">
                点端点立即断开并按它重连；地址、令牌、订阅会一起切换。
              </p>
            </div>
          </section>

          <!-- ── 显示方式 ─────────────────────── -->
          <section>
            <div class="sec-head">
              <span class="sec-title"><i />显示方式</span>
              <span class="chip alt">{{ displayMode === "topbar" ? "顶部灯条" : "右下角气泡" }}</span>
            </div>

            <div class="modes">
              <button
                class="mode"
                :class="{ on: displayMode === 'bubble' }"
                @click="setDisplayMode('bubble')"
              >
                <span class="mode-art bubble" aria-hidden="true" />
                <span class="mode-text">
                  <b>右下角气泡</b>
                  <em>带形象和进度条，多条叠着出现</em>
                </span>
              </button>

              <button
                class="mode"
                :class="{ on: displayMode === 'topbar' }"
                @click="setDisplayMode('topbar')"
              >
                <span class="mode-art bar" aria-hidden="true" />
                <span class="mode-text">
                  <b>顶部灯条</b>
                  <em>贴屏幕最上沿一行，不挡内容</em>
                </span>
              </button>
            </div>

            <p class="hint">两种形态吃同一份通知，随时可切。已收到、还没处理的会直接出现在新位置。</p>
          </section>

          <!-- ── 提示音 ───────────────────────── -->
          <section>
            <div class="sec-head">
              <span class="sec-title"><i />提示音</span>
              <span class="chip" :class="soundOn ? 'ok' : 'muted'">
                {{ soundOn ? `音量 ${Math.round(settings.sound_volume * 100)}%` : "已静音" }}
              </span>
            </div>

            <!-- 静音是这一段的第一件事：想关声音的人不该先读到「音量」再去理解开关 -->
            <div class="mute-row" :class="{ muted: !soundOn }" @click="toggleSound()">
              <MuteButton size="md" />
              <span class="mute-copy">
                <b>{{ soundOn ? "现在会出声" : "已静音，不会有任何声音" }}</b>
                <em>{{ soundOn ? "点一下立刻全部关掉" : "点一下恢复提示音" }}</em>
              </span>
              <kbd class="kbd">{{ MUTE_HOTKEY }}</kbd>
            </div>

            <label class="vol" :class="{ off: !settings.sound_enabled }">
              <span class="vol-label">音量</span>
              <input
                class="vol-range"
                type="range"
                min="0"
                max="100"
                step="5"
                :value="Math.round(settings.sound_volume * 100)"
                :disabled="!settings.sound_enabled"
                @input="onVolume"
                @change="onVolumeCommit"
              />
              <span class="vol-num">{{ Math.round(settings.sound_volume * 100) }}</span>
            </label>

            <div class="sfx" :class="{ off: !soundOn }">
              <button
                v-for="s in SFX_PREVIEW"
                :key="s.status"
                class="sfx-btn"
                :data-kind="s.status"
                :disabled="!soundOn"
                @click="previewSfx(s.status)"
              >
                <span class="sfx-icon">{{ s.icon }}</span>
                <span class="sfx-name">{{ s.label }}</span>
              </button>
            </div>

            <p class="hint">
              四种音同一套音色：开始最轻、成功最暖、警告敲两下、失败最沉。
              <template v-if="!soundOn">静音状态下试听也不会出声。</template>
            </p>
          </section>

          <!-- ── 本机身份 ─────────────────────── -->
          <section>
            <div class="sec-head">
              <span class="sec-title"><i />本机身份</span>
              <span class="chip" :class="{ ok: terminal }">
                {{ terminal ? "服务端已认下" : "尚未注册" }}
              </span>
            </div>

            <div class="identity">
              <div class="id-row">
                <span class="id-label">终端 ID</span>
                <code class="id-val">{{ terminal?.id || settings.terminal_id || "—" }}</code>
              </div>
              <div class="id-row">
                <span class="id-label">设备</span>
                <code class="id-val">{{ deviceId ? deviceId.slice(0, 8) + "…" : "—" }}</code>
              </div>
              <div v-if="terminal?.labels?.length" class="id-row">
                <span class="id-label">标签</span>
                <span class="chips">
                  <span v-for="l in terminal.labels" :key="l" class="chipmini">{{ l }}</span>
                </span>
              </div>
            </div>

            <label class="field">
              <input
                v-model="aliasDraft"
                class="input"
                type="text"
                spellcheck="false"
                autocomplete="off"
                placeholder="终端别名，留空则用计算机名"
                @change="saveIdentity"
              />
            </label>

            <label class="field">
              <input
                v-model="labelsDraft"
                class="input"
                type="text"
                spellcheck="false"
                autocomplete="off"
                placeholder="标签，逗号分隔，如 办公,win11"
                @change="saveIdentity"
              />
            </label>

            <p class="hint">
              别名和标签是你<b>报给服务端</b>的建议值，服务端可以覆盖。
              按标签分组建任务时，这台机器就靠标签被选中。
            </p>
          </section>

          <!-- ── 行为 ─────────────────────────── -->
          <section>
            <div class="sec-head">
              <span class="sec-title"><i />行为</span>
            </div>

            <button class="switch" role="switch" :aria-checked="settings.auto_connect" @click="toggleAuto">
              <span class="switch-text">
                <b>启动时自动连接</b>
                <em>关掉后只在点「连接」时才连</em>
              </span>
              <span class="track" :class="{ on: settings.auto_connect }"><i /></span>
            </button>

            <button
              v-if="mockVisible"
              class="switch"
              role="switch"
              :aria-checked="settings.mock_enabled"
              @click="toggleMock"
            >
              <span class="switch-text">
                <b>本机模拟数据</b>
                <em v-if="settings.server_url">已配服务端地址，模拟数据自动停用</em>
                <em v-else>没配服务端时用来预览</em>
              </span>
              <span class="track" :class="{ on: settings.mock_enabled }"><i /></span>
            </button>
          </section>

          <!-- ── 形象 ─────────────────────────── -->
          <section>
            <div class="sec-head">
              <span class="sec-title"><i />形象包</span>
            </div>

            <div class="packs">
              <button
                v-for="p in packs"
                :key="p.id"
                class="pack"
                :class="{ on: (activePackId ?? packs[0]?.id) === p.id }"
                @click="pickPack(p.id)"
              >
                <span class="pack-id">{{ p.id }}</span>
                <span class="pack-src">{{ p.source === "builtin" ? "内置" : "导入" }}</span>
              </button>
            </div>
            <p class="hint">服务端可以在通知里用 <code>buddy_pack</code> 指定某个任务用哪套。</p>

            <div class="forge">
              <span class="forge-title">定制自己的形象</span>
              <div class="forge-btns">
                <button class="btn ghost" @click="openPacks">打开形象包文件夹</button>
                <button class="btn ghost" @click="openGuide">定制指南 + 工具</button>
                <button class="btn ghost" @click="openApi">服务端接口文档</button>
              </div>
              <p v-if="forgeError" class="err">{{ forgeError }}</p>
              <p class="hint">
                「打开形象包文件夹」放的是你自己做的包；「定制指南 + 工具」是随安装包带的教程和脚本。
                做好的包拷进 <code>buddies\</code> 文件夹，重启即出现在上面的列表。
              </p>
            </div>
          </section>
          <!-- ── 调试（隐藏入口：底栏路径 3 秒内连点 5 次）── -->
          <section v-if="debugOpen">
            <div class="sec-head">
              <span class="sec-title"><i />调试 · 服务端原始帧</span>
              <span class="chip alt">{{ debugFrames.length }} 条</span>
            </div>

            <div class="row">
              <button class="btn ghost" @click="loadDebug">刷新</button>
              <button class="btn ghost" @click="copyDebug">复制全部</button>
              <button class="btn ghost" @click="clearDebug">清空</button>
              <button class="btn ghost" @click="debugOpen = false">关闭</button>
            </div>

            <div class="dbg">
              <div v-for="(f, i) in debugFrames" :key="f.ts + '-' + i" class="dbg-row">
                <span class="dbg-ts">{{ dbgTime(f.ts) }}</span>
                <span class="dbg-dir" :data-dir="f.dir">{{ f.dir }}</span>
                <pre class="dbg-text">{{ f.text }}</pre>
              </div>
              <p v-if="!debugFrames.length" class="hint">
                还没有记录。连接服务端后，这里会出现双方逐帧的原文。
              </p>
            </div>

            <p class="hint">
              最近 200 帧，只存内存，退出即清。← 服务端发来 · → 本机发出 · SYS 连接事件。
              帧里可能含 token，截图外发前注意。
            </p>
          </section>
        </div>

        <footer class="foot">
          <span class="foot-label">配置落在</span>
          <button class="path" :title="configPath || '浏览器预览不写文件'" @click="secretTap">
            {{ configPath || "localStorage（预览模式）" }}
            <span v-if="copied" class="copied">已复制</span>
          </button>
        </footer>
      </aside>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* ── 遮罩：比抽屉时代更暗更糊，玻璃卡才浮得出来 ── */
.veil {
  position: fixed;
  inset: 0;
  z-index: 80;
  background: rgba(3, 6, 12, 0.62);
  backdrop-filter: blur(6px);
}

/* ── 浮层模式（.float）：气泡同款玻璃卡，居中弹出 ──
   accent 随连接状态走；辉光、描边、主按钮全部从它派生。
   嵌入模式用的是 .page（见下），与此完全独立。 */
.float {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  z-index: 81;
  width: 560px;
  max-width: 94vw;
  max-height: 88vh;
  display: flex;
  flex-direction: column;
  border-radius: 18px;
  overflow: hidden;
  color: #dce6f6;
  background:
    radial-gradient(130% 150% at 0% 8%, color-mix(in srgb, var(--accent) 16%, transparent) 0%, transparent 56%),
    linear-gradient(158deg, rgba(24, 37, 59, 0.88) 0%, rgba(11, 18, 34, 0.92) 52%, rgba(7, 12, 22, 0.96) 100%);
  -webkit-backdrop-filter: blur(16px) saturate(1.15);
  backdrop-filter: blur(16px) saturate(1.15);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.1),
    0 30px 80px -24px color-mix(in srgb, var(--accent) 40%, transparent),
    0 24px 70px rgba(0, 0, 0, 0.6);
}

/* 斜切玻璃高光，气泡卡同款 */
.sheen {
  position: absolute;
  inset: -40% -10% 55% -30%;
  background: linear-gradient(
    104deg,
    transparent 30%,
    rgba(255, 255, 255, 0.06) 45%,
    rgba(255, 255, 255, 0.02) 52%,
    transparent 62%
  );
  pointer-events: none;
}

/* ── 头部：气泡 topline 同构（徽标 + 主副文字 + 状态 chip）── */
.head {
  position: relative;
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 18px 20px 14px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}

.badge {
  width: 30px;
  height: 30px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: 9px;
  color: #06121d;
  background: color-mix(in srgb, var(--accent) 88%, #ffffff);
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent) 55%, transparent);
}

.head-text {
  flex: 1;
  min-width: 0;
}

.head-text h2 {
  margin: 0;
  font-family: "Noto Serif SC", "Songti SC", "SimSun", serif;
  font-size: 17px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: #f2f7ff;
  text-shadow: 0 0 14px color-mix(in srgb, var(--accent) 34%, transparent);
}

.head-text p {
  margin: 3px 0 0;
  font-size: 10.5px;
  letter-spacing: 0.04em;
  color: #64748b;
}

.chip {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 10px;
  border-radius: 999px;
  font-size: 10.5px;
  letter-spacing: 0.02em;
  color: #93a3bb;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.07);
  white-space: nowrap;
}

.chip .dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 8px var(--accent);
}

.chip.ok {
  color: #7ff0c4;
  border-color: rgba(47, 217, 164, 0.28);
  background: rgba(47, 217, 164, 0.08);
}

.chip.muted {
  color: #ffc2b8;
  border-color: rgba(255, 138, 118, 0.3);
  background: rgba(255, 138, 118, 0.08);
}

.chip.alt {
  color: #9ec9ff;
}

.close {
  width: 26px;
  height: 26px;
  flex: none;
  border: 1px solid rgba(255, 255, 255, 0.09);
  border-radius: 8px;
  background: transparent;
  color: #7d8ca6;
  font-size: 12px;
  cursor: pointer;
  transition: background 140ms ease, color 140ms ease;
}

.close:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #eaf2ff;
}

/* ── 内容区 ─────────────────────────────── */
.body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

/* 区块：气泡式的软卡，accent 只出现在小方点上 */
section {
  display: flex;
  flex-direction: column;
  gap: 9px;
  padding: 13px 14px 14px;
  border-radius: 14px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(255, 255, 255, 0.025);
}

.sec-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.sec-title {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: #8b9ab4;
}

/* accent 小方点：气泡徽标的缩小版 */
.sec-title i {
  width: 8px;
  height: 8px;
  border-radius: 3px;
  background: var(--accent);
  box-shadow: 0 0 8px color-mix(in srgb, var(--accent) 60%, transparent);
}

/* ── 输入：accent 聚焦辉光 ───────────────── */
.field {
  display: block;
}

.input {
  width: 100%;
  padding: 9px 12px;
  font: inherit;
  font-size: 12px;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  color: #e8f1ff;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 10px;
  outline: none;
  transition: border-color 140ms ease, box-shadow 140ms ease;
}

.input::placeholder {
  color: #46536b;
}

.input:focus {
  border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 12%, transparent),
    0 0 14px color-mix(in srgb, var(--accent) 18%, transparent);
}

.hint {
  margin: 0;
  font-size: 11px;
  line-height: 1.6;
  color: #5d6c85;
}

.hint code {
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 10.5px;
  padding: 1px 5px;
  border-radius: 4px;
  color: #8fb6d8;
  background: rgba(127, 232, 255, 0.07);
}

/* 激活码入网：新用户的第一入口，收进一个轻量的展开区 */
.act-toggle {
  margin: 2px 0 4px;
}

.btn.slim {
  padding: 4px 10px;
  font-size: 11px;
}

.act-box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 6px 0 10px;
  padding: 10px 12px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.025);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
}

/* 服务端自述的入网指引：一块来自部署方的说明，客户端原样展示 */
.srv-info {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 10px;
  font-size: 11px;
}

.srv-name {
  color: #8fb6d8;
}

.srv-contact {
  color: #5d6c85;
}

.hint.ok {
  color: #7ff0c4;
}

/* ── 按钮：主按钮 = accent 胶囊（气泡 cta 同款）── */
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 2px;
}

.btn {
  font: inherit;
  font-size: 11.5px;
  padding: 7px 18px;
  border-radius: 999px;
  cursor: pointer;
  transition: filter 140ms ease, background 140ms ease, opacity 140ms ease;
}

.btn.primary {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 0;
  color: #06121d;
  font-weight: 600;
  background: color-mix(in srgb, var(--accent) 88%, #ffffff);
  box-shadow: 0 0 16px color-mix(in srgb, var(--accent) 45%, transparent);
}

.btn.primary:hover:not(:disabled) {
  filter: brightness(1.1);
}

.btn.primary:disabled {
  opacity: 0.4;
  cursor: not-allowed;
  box-shadow: none;
}

.btn.ghost {
  border: 1px solid rgba(255, 255, 255, 0.11);
  background: transparent;
  color: #9daec8;
}

.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.06);
  color: #eaf2ff;
}

.tag {
  font-size: 10px;
  color: #c79a4a;
}

.err {
  margin: 0;
  font-size: 11px;
  line-height: 1.6;
  color: #ff9aa5;
}

.err-sub {
  display: inline-block;
  margin-top: 3px;
  color: #8a6a72;
}

.warn {
  margin: 0;
  font-size: 11px;
  line-height: 1.6;
  color: #c79a4a;
}

/* ── 显示方式：两张缩略卡，选中描边走 accent ── */
.modes {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.mode {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 9px;
  font: inherit;
  text-align: left;
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
  cursor: pointer;
  transition: background 140ms ease, border-color 140ms ease, box-shadow 140ms ease;
}

.mode:hover {
  background: rgba(255, 255, 255, 0.05);
}

.mode.on {
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  background: color-mix(in srgb, var(--accent) 7%, transparent);
  box-shadow: 0 0 14px -4px color-mix(in srgb, var(--accent) 40%, transparent);
}

.mode-art {
  position: relative;
  display: block;
  height: 36px;
  border-radius: 6px;
  overflow: hidden;
  background: #060a12;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
}

.mode-art.bubble {
  background:
    radial-gradient(70% 70% at 82% 96%, color-mix(in srgb, var(--accent) 16%, transparent), transparent 72%),
    #060a12;
}

.mode-art.bubble::before {
  content: "";
  position: absolute;
  right: 5px;
  bottom: 5px;
  width: 27px;
  height: 16px;
  border-radius: 5px;
  background: linear-gradient(160deg, color-mix(in srgb, var(--accent) 32%, transparent), rgba(10, 18, 32, 0.92));
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent),
    0 4px 10px -4px color-mix(in srgb, var(--accent) 55%, transparent);
}

.mode-art.bubble::after {
  content: "";
  position: absolute;
  right: 34px;
  bottom: 7px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 9px var(--accent);
}

.mode-art.bar {
  background: linear-gradient(180deg, #0a1220 0%, #060a12 100%);
}

.mode-art.bar::before {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: 10px;
  background: #101a2b;
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, var(--accent) 90%, transparent),
    inset 0 -1px 0 rgba(255, 255, 255, 0.07),
    0 6px 14px -8px color-mix(in srgb, var(--accent) 75%, transparent);
}

.mode-art.bar::after {
  content: "";
  position: absolute;
  left: 7px;
  top: 3.5px;
  width: 18px;
  height: 3px;
  border-radius: 2px;
  background: color-mix(in srgb, var(--accent) 45%, transparent);
  box-shadow: 22px 0 0 rgba(255, 255, 255, 0.16);
}

.mode-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.mode-text b {
  font-size: 12px;
  font-weight: 500;
  color: #dbe6f6;
}

.mode-text em {
  font-size: 10px;
  font-style: normal;
  line-height: 1.4;
  color: #5d6c85;
}

/* ── 提示音 ─────────────────────────────── */
.mute-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
  cursor: pointer;
  transition: background 140ms ease, border-color 140ms ease;
}

.mute-row:hover {
  background: rgba(255, 255, 255, 0.05);
}

.mute-row.muted {
  border-color: rgba(255, 138, 118, 0.3);
  background: rgba(255, 138, 118, 0.08);
}

.mute-copy {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
}

.mute-copy b {
  font-size: 12px;
  font-weight: 500;
  color: #dbe6f6;
}

.mute-copy em {
  font-size: 10.5px;
  font-style: normal;
  color: #5d6c85;
}

.mute-row.muted .mute-copy b {
  color: #ffd2ca;
}

.kbd {
  flex: none;
  padding: 2px 6px;
  border-radius: 5px;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 9.5px;
  color: #6c7c98;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.07);
  white-space: nowrap;
}

.vol.off,
.sfx.off {
  opacity: 0.5;
}

.vol {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
}

.vol-label {
  flex: none;
  font-size: 12px;
  color: #dbe6f6;
}

.vol-num {
  flex: none;
  width: 3em;
  text-align: right;
  font-size: 11px;
  color: #8e9cb5;
  font-variant-numeric: tabular-nums;
}

.vol-range {
  flex: 1;
  min-width: 0;
  height: 18px;
  accent-color: var(--accent);
  background: transparent;
  cursor: pointer;
}

.vol-range:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.sfx {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 8px;
}

.sfx-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  padding: 9px 2px 8px;
  font: inherit;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
  cursor: pointer;
  transition: background 140ms ease, transform 90ms ease;
}

.sfx-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.055);
}

.sfx-btn:active:not(:disabled) {
  transform: scale(0.96);
}

.sfx-btn:disabled {
  cursor: not-allowed;
}

.sfx-icon {
  font-size: 13px;
  line-height: 1;
  color: #8e9cb5;
}

.sfx-name {
  font-size: 10.5px;
  color: #b9c6da;
}

.sfx-btn[data-kind="running"] .sfx-icon {
  color: #5fb8ff;
}

.sfx-btn[data-kind="success"] .sfx-icon {
  color: #2fd9a4;
}

.sfx-btn[data-kind="warning"] .sfx-icon {
  color: #f2b34b;
}

.sfx-btn[data-kind="failed"] .sfx-icon {
  color: #f2707a;
}

/* ── 本机身份 ───────────────────────────── */
.identity {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(255, 255, 255, 0.015);
}

.id-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 16px;
}

.id-label {
  flex: none;
  width: 52px;
  font-size: 10.5px;
  color: #5d6c85;
}

.id-val {
  flex: 1;
  min-width: 0;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 10.5px;
  color: #9fb6d2;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.chipmini {
  padding: 1px 7px;
  font-size: 10px;
  border-radius: 999px;
  color: color-mix(in srgb, var(--accent) 70%, #cfe8ff);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
}

/* ── 开关：track.on 走 accent ────────────── */
.switch {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  width: 100%;
  padding: 10px 12px;
  font: inherit;
  text-align: left;
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
  cursor: pointer;
  transition: background 140ms ease;
}

.switch:hover {
  background: rgba(255, 255, 255, 0.05);
}

.switch-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.switch-text b {
  font-size: 12px;
  font-weight: 500;
  color: #dbe6f6;
}

.switch-text em {
  font-size: 10.5px;
  font-style: normal;
  color: #5d6c85;
}

.track {
  flex: none;
  width: 34px;
  height: 19px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.08);
  position: relative;
  transition: background 180ms ease;
}

.track i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: #8296b3;
  transition: transform 180ms cubic-bezier(0.22, 0.61, 0.36, 1), background 180ms ease;
}

.track.on {
  background: color-mix(in srgb, var(--accent) 28%, transparent);
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
}

.track.on i {
  transform: translateX(15px);
  background: color-mix(in srgb, var(--accent) 85%, #ffffff);
  box-shadow: 0 0 8px color-mix(in srgb, var(--accent) 70%, transparent);
}

/* ── 形象包 ─────────────────────────────── */
.packs {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.pack {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 9px 11px;
  font: inherit;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
  color: #a7b6cd;
  cursor: pointer;
  transition: background 140ms ease, border-color 140ms ease, color 140ms ease;
}

.pack:hover {
  background: rgba(255, 255, 255, 0.055);
}

.pack.on {
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  color: #e2f6ff;
}

.pack-id {
  font-size: 12px;
}

.pack-src {
  font-size: 9.5px;
  letter-spacing: 0.04em;
  color: #5d6c85;
}

.forge {
  display: flex;
  flex-direction: column;
  gap: 9px;
  margin-top: 2px;
  padding: 11px 12px;
  border-radius: 12px;
  border: 1px dashed rgba(255, 255, 255, 0.12);
  background: rgba(255, 255, 255, 0.012);
}

.forge-title {
  font-size: 11.5px;
  font-weight: 500;
  color: #a7b6cd;
}

.forge-btns {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
}

.forge-btns .btn {
  font-size: 10.5px;
  padding: 5px 11px;
}

/* ── 底栏 ───────────────────────────────── */
.foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 11px 20px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(0, 0, 0, 0.28);
}

.foot-label {
  flex: none;
  font-size: 10px;
  color: #4d5a71;
}

.path {
  flex: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  background: none;
  font-size: 10px;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  color: #6d7f99;
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

.path:hover {
  color: #9fbbe0;
}

.copied {
  margin-left: 6px;
  font-family: "Inter", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;
  color: #2fd9a4;
}

/* ── 过渡：居中弹出，与气泡入场同一族曲线 ── */
.veil-enter-active,
.veil-leave-active {
  transition: opacity 180ms ease;
}

.veil-enter-from,
.veil-leave-to {
  opacity: 0;
}

.pop-enter-active {
  transition: transform 220ms cubic-bezier(0.22, 0.61, 0.36, 1), opacity 160ms ease;
}

.pop-leave-active {
  transition: transform 150ms ease, opacity 140ms ease;
}

.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translate(-50%, -46%) scale(0.97);
}

/* ── 嵌入模式（.page）：设置就是窗口本身 ──
   不透明背景、无圆角、无阴影、无定位 —— 和浮层卡片(.float)完全两套，
   从根上避免「窗口里再浮一张卡」的套娃观感。 */
.page {
  position: relative;
  width: 100%;
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  color: #dce6f6;
  background:
    radial-gradient(130% 150% at 0% 8%, color-mix(in srgb, var(--accent) 10%, transparent) 0%, transparent 56%),
    linear-gradient(158deg, #16233a 0%, #0b1222 52%, #070c16 100%);
}

/* 窗口可能很宽，内容居中限宽，别让输入框拉到 1000px */
.page .body {
  align-items: center;
}

.page .body > section {
  width: min(680px, 100%);
}

/* ── 订阅类型：折叠的高级项 ──────────────── */
.adv {
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.015);
}

.adv summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 9px 12px;
  font-size: 12px;
  color: #dbe6f6;
  cursor: pointer;
  list-style: none;
  border-radius: 10px;
  transition: background 140ms ease;
}

.adv summary::-webkit-details-marker {
  display: none;
}

.adv summary:hover {
  background: rgba(255, 255, 255, 0.04);
}

/* 展开指示箭头：闭合朝下，展开转向上 */
.adv summary::after {
  content: "▾";
  flex: none;
  font-size: 10px;
  color: #5d6c85;
  transition: transform 160ms ease;
}

.adv[open] summary::after {
  transform: rotate(180deg);
}

.adv summary em {
  flex: 1;
  font-style: normal;
  font-size: 10.5px;
  text-align: right;
  color: #5d6c85;
}

.adv-body {
  display: flex;
  flex-direction: column;
  gap: 9px;
  padding: 2px 12px 12px;
}

/* ── 本地演示服务端 ─────────────────────── */
.local-demo {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 2px;
}

.local-status {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 11px;
  color: #7fe7bd;
}

.local-status i {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #2fd9a4;
  box-shadow: 0 0 8px rgba(47, 217, 164, 0.7);
  animation: pulse 1.6s ease-in-out infinite;
}

.local-tip {
  font-size: 11px;
  color: #6c7c98;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
}

/* ── 服务端 ─────────────────────────────── */
.ep-block {
  display: flex;
  flex-direction: column;
  gap: 9px;
  margin-top: 6px;
  padding-top: 12px;
  border-top: 1px dashed rgba(255, 255, 255, 0.1);
}

.ep-guide {
  margin: 0;
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid rgba(47, 217, 164, 0.18);
  background: rgba(47, 217, 164, 0.06);
  font-size: 11px;
  line-height: 1.7;
  color: #9fd9c0;
}

.ep-guide b {
  color: #2fd9a4;
}

.ep-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.ep-title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  color: #8b9ab4;
}

/* ── 服务端点 ───────────────────────────── */
.ep-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.ep {
  display: flex;
  align-items: stretch;
  gap: 6px;
}

.ep-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  padding: 8px 12px;
  font: inherit;
  text-align: left;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(255, 255, 255, 0.02);
  cursor: pointer;
  transition: background 140ms ease, border-color 140ms ease, box-shadow 140ms ease;
}

.ep-main:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.05);
}

.ep-main:disabled {
  opacity: 0.5;
  cursor: wait;
}

.ep-main b {
  font-size: 12px;
  font-weight: 500;
  color: #dbe6f6;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.ep-main em {
  font-size: 10px;
  font-style: normal;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  color: #5d6c85;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.ep.on {
  border-radius: 10px;
}

.ep.on .ep-main {
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  box-shadow: 0 0 12px -4px color-mix(in srgb, var(--accent) 45%, transparent);
}

.ep.on .ep-main b {
  color: #eaf6ff;
}

.ep.on .ep-main em {
  color: color-mix(in srgb, var(--accent) 45%, #9fb6d2);
}

.ep-del {
  flex: none;
  width: 28px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  border-radius: 10px;
  background: transparent;
  color: #5d6c85;
  font-size: 11px;
  cursor: pointer;
  transition: background 140ms ease, color 140ms ease;
}

.ep-del:hover:not(:disabled) {
  background: rgba(255, 77, 94, 0.12);
  border-color: rgba(255, 77, 94, 0.3);
  color: #ff9aa5;
}

.ep-add {
  display: flex;
  gap: 8px;
}

.ep-add .input {
  flex: 1;
  min-width: 0;
}

.ep-add .btn {
  flex: none;
  font-size: 10.5px;
  padding: 5px 12px;
}

/* ── 调试区：等宽、紧凑、默认不存在 ──────── */
.dbg {
  max-height: 260px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 9px;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(0, 0, 0, 0.28);
}

.dbg-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  font-size: 10px;
}

.dbg-ts {
  flex: none;
  color: #4d5a71;
  font-variant-numeric: tabular-nums;
}

.dbg-dir {
  flex: none;
  min-width: 22px;
  text-align: center;
}

.dbg-dir[data-dir="←"] {
  color: #7ff0c4;
}

.dbg-dir[data-dir="→"] {
  color: #9ec9ff;
}

.dbg-dir[data-dir="SYS"] {
  color: #c79a4a;
}

.dbg-text {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 10px;
  line-height: 1.5;
  color: #a7b6cd;
  white-space: pre-wrap;
  word-break: break-all;
}

@media (prefers-reduced-motion: reduce) {
  .pop-enter-active,
  .pop-leave-active,
  .veil-enter-active,
  .veil-leave-active {
    transition-duration: 1ms;
  }
}
</style>
