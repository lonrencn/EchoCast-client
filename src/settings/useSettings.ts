/**
 * 全局设置 + 服务端连接状态。
 *
 * 和 notices.ts 一样是模块级单例：设置面板、状态灯、形象包切换器
 * 必须读同一份状态，否则会出现「面板显示已保存、气泡还在用旧包」。
 *
 * 桌面端把设置写到 $APPDATA/settings.json，浏览器预览退化成 localStorage ——
 * 预览环境没有 Rust 侧，写文件这件事根本不存在。
 */

import { computed, ref } from "vue";

/** 通知以什么形态出现。两种形态吃同一份 TaskNotice，服务端不需要知道。 */
export type DisplayMode = "bubble" | "topbar";

/** 一条保存过的服务端点。切换 = 整体覆盖顶层的 url/token/topics。 */
export interface EndpointProfile {
  id: string;
  label: string;
  url: string;
  token: string;
  topics: string;
}

export interface AppSettings {
  /** 服务端 WebSocket 地址。空串表示没配过。 */
  server_url: string;
  auto_connect: boolean;
  /** 本机 mock 推送，仅 debug 构建有效。 */
  mock_enabled: boolean;
  /** 选中的形象包 id，空串表示用内置 primary。 */
  buddy_pack: string;
  protocol: string;
  /** WS 握手用的 Bearer token，空串表示服务端不要求鉴权。 */
  token: string;
  /** 订阅的 topic，逗号分隔。服务端不做通配。 */
  topics: string;
  /** 本机终端别名，注册时上报。空串则用计算机名。 */
  terminal_alias: string;
  /** 本机标签，逗号分隔。服务端按标签路由时会用到。 */
  terminal_labels: string;
  /** 服务端签发的 terminal_id 缓存，只用于展示。 */
  terminal_id: string;
  /** 右下角气泡 / 顶部灯条。 */
  display_mode: DisplayMode;
  /** 提示音开关。关掉后四种音全静音，通知照常落。 */
  sound_enabled: boolean;
  /** 提示音音量 0–1。外放和投屏场合差得远，所以要能调。 */
  sound_volume: number;
  /** 保存过的服务端点。顶层 url/token/topics 是当前生效值。 */
  endpoints: EndpointProfile[];
  /** 当前生效的端点 id，空串表示当前配置不属于任何端点。 */
  active_endpoint: string;
}

/** 服务端在 hello_ack 里认下的终端身份。 */
export interface TerminalInfo {
  id: string;
  alias: string;
  labels: string[];
}

/** 与 src-tauri/src/conn.rs 的 ConnStatus 一一对应。 */
export type ConnStatus =
  | { state: "idle" }
  | { state: "connecting"; url: string }
  | { state: "connected"; url: string; since: number }
  | { state: "error"; message: string; retry_in: number }
  /** 配置层错误（token 无效 / 已吊销）。已经停止重连，必须用户动手。 */
  | { state: "fatal"; message: string };

const DEFAULTS: AppSettings = {
  server_url: "",
  auto_connect: true,
  mock_enabled: true,
  buddy_pack: "",
  protocol: "1",
  token: "",
  // 清单来自服务端 BROADCAST v1.1：服务端不做通配，漏一个就少一路通知
  topics:
    "task.started,task.running,task.completed,task.failed,task.warning," +
    "alert.critical,alert.warn,deploy.done," +
    "flimx.project.updated,flimx.asset.missing,flimx.subtitle.generated",
  terminal_alias: "",
  terminal_labels: "",
  terminal_id: "",
  display_mode: "bubble",
  sound_enabled: true,
  sound_volume: 0.7,
  endpoints: [],
  active_endpoint: "",
};

const LS_KEY = "echocast.settings";

/** 默认订阅清单（与服务端 BROADCAST v1.1 对齐）。设置面板的「恢复默认订阅」回到它。 */
export const DEFAULT_TOPICS = DEFAULTS.topics;

export const settings = ref<AppSettings>({ ...DEFAULTS });
export const conn = ref<ConnStatus>({ state: "idle" });
/** 服务端认下的身份。连上但服务端还没回 hello_ack 时为 null。 */
export const terminal = ref<TerminalInfo | null>(null);

/** 内置本地演示服务端的状态。null = 还没启动过。 */
export interface LocalDemoStatus {
  running: boolean;
  port: number;
  clients: number;
}
export const localDemo = ref<LocalDemoStatus | null>(null);
export const configPath = ref("");
export const settingsReady = ref(false);
export const settingsError = ref("");
export const busy = ref(false);

/** 浏览器预览里没有 IPC，所有连接能力都不可用，界面要如实说明。 */
export const desktop = computed(
  () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
);

export const isConnected = computed(() => conn.value.state === "connected");

/**
 * 当前通知形态。
 *
 * 旧版本写下的 settings.json 里没有这个字段，读出来是 undefined ——
 * 这里兜一次底，不让 `undefined` 参与模板判断。
 */
export const displayMode = computed<DisplayMode>(() =>
  settings.value.display_mode === "topbar" ? "topbar" : "bubble"
);

/**
 * 切换形态。只改展示，不动数据源：
 * 切过去时手上还没处理的那几条会直接出现在新位置，不会被清掉。
 */
export async function setDisplayMode(mode: DisplayMode): Promise<void> {
  if (displayMode.value === mode) return;
  await saveSettings({ display_mode: mode });
  await applyOverlay();
  // 切完立刻广播一条「欢迎」：新形态当场在自己位置弹一条，
  // 用户马上知道「通知以后从哪冒出来」，不用等第一条真任务。
  if (desktop.value) {
    try {
      const { emit } = await import("@tauri-apps/api/event");
      await emit("ec:welcome");
    } catch {
      // 预览弹不出来只是少个演示，不值得报错
    }
  }
}

/**
 * 把 display_mode 应用到桌面悬浮窗口（显示/隐藏、摆位）。
 *
 * 只该由主窗口调用：悬浮窗口自己没有设置面板，也不知道对方存在。
 * 启动时和切换形态时各调一次；失败静默 —— 窗口创建失败顶多回到
 * 主窗口内预览，不值得弹错打扰人。
 */
export async function applyOverlay(): Promise<void> {
  if (!desktop.value) return;
  try {
    const invoke = await tauri();
    await invoke("overlay_apply", { mode: displayMode.value });
  } catch (err) {
    console.warn("[settings] 悬浮窗口调整失败", err);
  }
}

/**
 * 现在会不会出声。开关关掉、或音量拖到 0，都算静音 ——
 * 界面上凡是「能不能响」的判断都读它，不要各处自己拼条件。
 */
export const soundOn = computed(
  () => settings.value.sound_enabled && settings.value.sound_volume > 0.001
);

/** 静音快捷键。写在启动时注册的那个监听里，也从这里取，避免两处写死不一致。 */
export const MUTE_HOTKEY = "Ctrl+Shift+M";

/**
 * 静音 / 取消静音。
 *
 * 先改本地状态再落盘：写 settings.json 是异步的，等它回来才生效的话，
 * 用户按下开关到耳朵安静之间会多出几十毫秒甚至还响一声 —— 那一刻他只会觉得「没反应」。
 * 真的播放拦截在 sfx.ts 里读的是同一份 `settings`，所以本地改完就已经哑了。
 */
export async function setSoundMuted(muted: boolean): Promise<boolean> {
  const next = !muted;
  const volZero = settings.value.sound_volume <= 0.001;
  if (settings.value.sound_enabled === next && !(next && volZero)) return true;
  settings.value.sound_enabled = next;
  // 取消静音时音量还停在 0，等于没解开 —— 用户会以为开关是坏的。
  // 「打开声音」这个动作必须真的出声，顺手把音量捡回可用值。
  if (next && volZero) {
    settings.value.sound_volume = 0.6;
    return saveSettings({ sound_enabled: true, sound_volume: 0.6 });
  }
  return saveSettings({ sound_enabled: next });
}

export async function toggleSound(): Promise<boolean> {
  // 按「会不会出声」翻转，而不是开关本身：
  // 开关开着但音量为 0 时，界面显示的是静音，点一下必须真的出声 ——
  // 若按开关翻转，这次点击只会把已经开着的开关关掉，声音照样没有，
  // 用户看到的就是「点了没反应，打不开声音」。
  return setSoundMuted(soundOn.value);
}

/** 状态灯颜色分档。`error` 和「正在重连」必须区分开，否则用户以为已经放弃。 */
export const connTone = computed<"off" | "wait" | "on" | "bad">(() => {  switch (conn.value.state) {
    case "connected":
      return "on";
    case "connecting":
      return "wait";
    case "error":
    case "fatal":
      return "bad";
    default:
      return "off";
  }
});

export const connText = computed(() => {
  const s = conn.value;
  switch (s.state) {
    case "connected":
      return "已连接";
    case "connecting":
      return "正在连接";
    case "error":
      return s.retry_in > 0 ? `连接失败，${s.retry_in} 秒后重试` : "连接失败";
    case "fatal":
      // 已经停止重连了，文案必须说明「需要你动手」，否则用户会一直等
      return "需要重新配置";
    default:
      return "未连接";
  }
});

/** 致命错误只在设置面板里展开说，状态灯上放不下也不该放。 */
export const connFatal = computed(() =>
  conn.value.state === "fatal" ? conn.value.message : ""
);

async function tauri() {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke;
}

/** 只跑一次。App 启动时调用，晚到的组件读现成的 ref。 */
let loading: Promise<void> | null = null;

export function loadSettings(): Promise<void> {
  if (loading) return loading;

  loading = (async () => {
    if (!desktop.value) {
      try {
        const raw = localStorage.getItem(LS_KEY);
        if (raw) settings.value = { ...DEFAULTS, ...JSON.parse(raw) };
      } catch {
        // 存的东西坏了就当没存过，不值得为它报错
      }
      settingsReady.value = true;
      return;
    }

    try {
      const invoke = await tauri();
      settings.value = await invoke<AppSettings>("settings_get");
      configPath.value = (await invoke<string | null>("settings_path")) ?? "";
      conn.value = await invoke<ConnStatus>("conn_status");
      terminal.value = await invoke<TerminalInfo | null>("terminal_get");

      // 订阅放在读完状态之后：先订阅后读会出现「先收到新状态、再被旧快照覆盖」
      const { listen } = await import("@tauri-apps/api/event");
      await listen<ConnStatus>("ec:conn", (e) => {
        conn.value = e.payload;
        // 断开就要把身份清掉：继续显示「我是书房 Windows」会让人以为还连着，
        // 而这时候服务端根本不会把任务发过来。
        if (e.payload.state === "idle" || e.payload.state === "error" || e.payload.state === "fatal") {
          terminal.value = null;
        }
      });
      await listen<TerminalInfo>("ec:terminal", (e) => {
        terminal.value = e.payload;
      });
      // 托盘菜单里的静音开关在 Rust 侧翻的，这里把结果同步回来，
      // 否则面板上的开关和托盘各说各话。
      await listen<Partial<AppSettings>>("ec:settings-updated", (e) => {
        settings.value = { ...settings.value, ...e.payload };
      });
      // 本地演示服务端的连接数变化由 Rust 推过来，面板上的状态条实时跟着走
      await listen<LocalDemoStatus>("ec:localdemo", (e) => {
        localDemo.value = e.payload;
      });
    } catch (err) {
      settingsError.value = `读取设置失败：${String(err)}`;
    }

    settingsReady.value = true;
  })();

  return loading;
}

export async function saveSettings(patch: Partial<AppSettings>): Promise<boolean> {
  const next = { ...settings.value, ...patch };

  if (!desktop.value) {
    settings.value = next;
    localStorage.setItem(LS_KEY, JSON.stringify(next));
    return true;
  }

  try {
    const invoke = await tauri();
    // 后端会把地址规范化后回传，这里必须用返回值覆盖 ——
    // 用户填了 `127.0.0.1:8766`，界面上应该变成 `ws://127.0.0.1:8766`。
    settings.value = await invoke<AppSettings>("settings_set", { settings: next });
    settingsError.value = "";
    return true;
  } catch (err) {
    settingsError.value = String(err);
    return false;
  }
}

/** 连接并记住这个地址。地址不合法时后端会抛错，界面拿它显示原因。 */
export async function connectServer(url: string): Promise<boolean> {
  if (!desktop.value) {
    settingsError.value = "浏览器预览没有连接能力，请在桌面端操作";
    return false;
  }
  busy.value = true;
  try {
    const invoke = await tauri();
    const normalized = await invoke<string>("conn_connect", { url });
    await saveSettings({ server_url: normalized });
    settingsError.value = "";
    return true;
  } catch (err) {
    settingsError.value = String(err);
    return false;
  } finally {
    busy.value = false;
  }
}

export async function disconnectServer(): Promise<void> {
  if (!desktop.value) return;
  const invoke = await tauri();
  await invoke("conn_disconnect");
}

// ---------------------------------------------------------------------------
// 本地演示服务端：第一次使用的引导入口
// ---------------------------------------------------------------------------

/**
 * 启动内置演示服务端并自动连上。
 *
 * 给手里还没有服务端的人用：不用先去弄懂部署，点一下就能看到
 * 通知长什么样。重复调用幂等（Rust 侧已在跑就只回报状态）。
 */
export async function startLocalDemo(): Promise<boolean> {
  if (!desktop.value) {
    settingsError.value = "浏览器预览没有连接能力，请在桌面端操作";
    return false;
  }
  busy.value = true;
  try {
    const invoke = await tauri();
    const st = await invoke<LocalDemoStatus>("localdemo_start");
    localDemo.value = st;
    // 连接直接切到演示服务端。不动已保存的端点列表 ——
    // 用户之后连正式环境，「添加服务端点」照常工作。
    return await connectServer(`ws://127.0.0.1:${st.port}/ws`);
  } catch (err) {
    settingsError.value = String(err);
    return false;
  } finally {
    busy.value = false;
  }
}

// ---------------------------------------------------------------------------
// 服务端点管理：多环境切换
// ---------------------------------------------------------------------------

/** 当前生效的三件套是否和某个已存端点完全一致。 */
export function findMatchingEndpoint(): EndpointProfile | undefined {
  const s = settings.value;
  return s.endpoints.find(
    (e) =>
      e.url === s.server_url && e.token === s.token && e.topics === s.topics
  );
}

/** 把当前生效的 url/token/topics 存成一条端点。已有一致的则只改名字。 */
export async function saveCurrentAsEndpoint(label: string): Promise<boolean> {
  const s = settings.value;
  if (!s.server_url.trim()) {
    settingsError.value = "还没有可保存的服务端地址";
    return false;
  }
  const exist = findMatchingEndpoint();
  const next = exist
    ? s.endpoints.map((e) => (e.id === exist.id ? { ...e, label: label.trim() || e.label } : e))
    : [
        ...s.endpoints,
        {
          id: `ep-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
          label: label.trim(),
          url: s.server_url,
          token: s.token,
          topics: s.topics,
        },
      ];
  const ok = await saveSettings({
    endpoints: next,
    active_endpoint: exist ? exist.id : next[next.length - 1].id,
  });
  return ok;
}

/** 切换端点：三件套整体覆盖到顶层，随后由用户决定何时连接。 */
export async function activateEndpoint(id: string): Promise<boolean> {
  const ep = settings.value.endpoints.find((e) => e.id === id);
  if (!ep) return false;
  return saveSettings({
    server_url: ep.url,
    token: ep.token,
    topics: ep.topics || DEFAULTS.topics,
    active_endpoint: id,
  });
}

/** 切到端点后立刻断开旧连接并按新配置连接。失败返回 false 并写 settingsError。 */
export async function switchEndpointNow(id: string): Promise<boolean> {
  const ep = settings.value.endpoints.find((e) => e.id === id);
  if (!ep) return false;
  if (!desktop.value) return activateEndpoint(id);
  await disconnectServer();
  const ok = await activateEndpoint(id);
  if (!ok) return false;
  return connectServer(ep.url);
}

/** 删除端点。删的是当前生效那条时，只解除指向，连接本身不动。 */
export async function removeEndpoint(id: string): Promise<boolean> {
  const s = settings.value;
  return saveSettings({
    endpoints: s.endpoints.filter((e) => e.id !== id),
    active_endpoint: s.active_endpoint === id ? "" : s.active_endpoint,
  });
}

/** 给端点改名字。 */
export async function renameEndpoint(id: string, label: string): Promise<boolean> {
  const s = settings.value;
  return saveSettings({
    endpoints: s.endpoints.map((e) => (e.id === id ? { ...e, label: label.trim() } : e)),
  });
}

/**
 * 把用户点的动作回传给服务端。
 *
 * 服务端定的是 WS 上行帧 `{op:"action", notice_id, action_id, payload}`，
 * 复用同一条连接。没连上时不假装成功 —— 返回 false 让调用方决定怎么提示。
 */
export async function sendAction(
  noticeId: string,
  actionId: string,
  payload?: string
): Promise<{ ok: boolean; reason?: string }> {
  if (!desktop.value) return { ok: false, reason: "浏览器预览没有连接能力" };
  if (!isConnected.value) return { ok: false, reason: "还没连上服务端" };
  try {
    const invoke = await tauri();
    await invoke("conn_send_action", {
      noticeId,
      actionId,
      payload: payload ?? null,
    });
    return { ok: true };
  } catch (err) {
    return { ok: false, reason: String(err) };
  }
}

/**
 * 任务级回执。
 *
 * 和 `sendAction` 的区别：`action` 是「用户点了某个按钮」，`ack` 是
 * 「这条任务到哪个阶段了」。服务端做投递看板时只关心后者 —— 一条任务
 * 可能有动作按钮，也可能没有，但每个任务都必须有 ack 状态。
 *
 * `seen` 不从这里发，Rust 收到投递就自动回了。
 */
export async function sendAck(
  taskId: string,
  state: "accepted" | "rejected" | "done",
  reason?: string
): Promise<{ ok: boolean; reason?: string }> {
  if (!desktop.value) return { ok: false, reason: "浏览器预览没有连接能力" };
  if (!isConnected.value) return { ok: false, reason: "还没连上服务端" };
  try {
    const invoke = await tauri();
    await invoke("conn_send_ack", { taskId, state, reason: reason ?? null });
    return { ok: true };
  } catch (err) {
    return { ok: false, reason: String(err) };
  }
}
