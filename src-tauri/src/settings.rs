//! 用户设置，落在 `$APPDATA/settings.json`。
//!
//! 为什么不放前端 localStorage：WebSocket 连接由 Rust 侧发起，
//! 它读不到 WebView 的存储。设置必须存在一个两个进程都能读到的地方，
//! 否则「界面上填了地址、实际没连」这种鬼故事一定会发生。
//!
//! 读取时对缺字段一律回落默认值，这样旧版本写下的文件不会让新版本启动失败。

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// 一条可切换的服务端点。`url/token/topics` 三件套打包存，
/// 切换时整体覆盖到顶层的同名字段 —— 连接层（conn.rs）只认顶层，不用改。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointProfile {
    /// 稳定 id，用 uuid 前缀即可，仅用于前端 key 和 active 指向。
    pub id: String,
    /// 用户起的名字，如「公司网关」「本机调试」。空串则显示地址。
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub topics: String,
}

/// 全部字段都给默认值：新增字段不破坏老配置文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// 服务端地址。空串表示没配过，此时不会尝试连接。
    pub server_url: String,
    /// 启动后自动连接。
    pub auto_connect: bool,
    /// 本机 mock 推送开关（只在 debug 构建里有效）。
    pub mock_enabled: bool,
    /// 选中的形象包 id，空串表示跟随内置 primary。
    pub buddy_pack: String,
    /// 协议版本，服务端不匹配时前端据此提示而不是静默失败。
    pub protocol: String,
    /// WS 握手用的 Bearer token。空串表示服务端不要求鉴权。
    ///
    /// 这个 token 与**一台**终端绑定，不是「入网令牌」——
    /// 服务端靠它反查 terminal_id 才知道该把哪些任务发给这条连接。
    pub token: String,
    /// 订阅的 topic 列表，逗号分隔。服务端不做通配，必须逐个列出。
    pub topics: String,
    /// 本机终端别名，注册时上报，服务端可覆盖。空串则用 hostname。
    pub terminal_alias: String,
    /// 本机标签，逗号分隔，注册时上报。
    /// 这是「建议值」：服务端管理员可以改，下次 hello_ack 会覆盖回来。
    pub terminal_labels: String,
    /// 服务端签发的 terminal_id 缓存，只用于展示。
    /// 空串表示还没跟服务端对过号（没连过，或服务端不支持 hello_ack）。
    pub terminal_id: String,
    /// 通知形态：`bubble`（右下角气泡）或 `topbar`（贴屏顶部灯条）。
    ///
    /// 这是**纯展示偏好**，不影响协议：两种形态吃的是同一份 `TaskNotice`，
    /// 服务端不需要知道终端选了哪种，也不该为此多推一个字段。
    pub display_mode: String,
    /// 提示音开关。关掉后四种音全静音，但通知照常落。
    pub sound_enabled: bool,
    /// 提示音音量，0.0–1.0。0 等价于静音，与 sound_enabled 双保险。
    ///
    /// 存在的意义不是「让用户调大小」，而是笔记本外放和会议室投屏之间
    /// 差着二十几 dB：同一个音量在两种场合要么听不见，要么吓人一跳。
    pub sound_volume: f32,
    /// 保存过的服务端点列表。顶层 server_url/token/topics 始终是
    /// 「当前生效」的值；这里只是方便用户在多个环境之间切换。
    pub endpoints: Vec<EndpointProfile>,
    /// 当前生效的端点 id。空串表示当前配置不属于任何已存端点。
    pub active_endpoint: String,
    /// v2 登记后 pin 住的服务端标识符。重连时随 X-Server-Id 头回传，
    /// 不匹配会被服务端 close 4004——防令牌误指到别的服务器。
    /// 空表示该端点是 v1 签发制（无 pin）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    /// v2 登记时本地自生成的设备标识符（明文只在本机）。
    /// 轮换 = 本地重新生成后重新登记；换服务器登记时复用同一份。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_token: Option<String>,
}

/// 服务端的订阅 topic 没有通配，漏一个就少一路通知。
/// 清单来自服务端 BROADCAST v1.1（`c973fa0`），改之前先跟服务端核对。
pub const DEFAULT_TOPICS: &str = concat!(
    "task.started,task.running,task.completed,task.failed,task.warning,",
    "alert.critical,alert.warn,deploy.done,",
    "flimx.project.updated,flimx.asset.missing,flimx.subtitle.generated"
);

/// 右下角气泡，默认档。
pub const DISPLAY_BUBBLE: &str = "bubble";
/// 贴屏顶部的单行灯条。
pub const DISPLAY_TOPBAR: &str = "topbar";

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            auto_connect: true,
            mock_enabled: true,
            buddy_pack: String::new(),
            protocol: "1".into(),
            token: String::new(),
            topics: DEFAULT_TOPICS.into(),
            terminal_alias: String::new(),
            terminal_labels: String::new(),
            terminal_id: String::new(),
            display_mode: DISPLAY_BUBBLE.into(),
            sound_enabled: true,
            sound_volume: 0.7,
            endpoints: Vec::new(),
            active_endpoint: String::new(),
            server_id: None,
            device_token: None,
        }
    }
}

impl AppSettings {
    /// 存盘前做一次收敛：去掉首尾空白，把空地址统一成空串。
    pub fn normalized(mut self) -> Self {
        self.server_url = self.server_url.trim().to_string();
        self.buddy_pack = self.buddy_pack.trim().to_string();
        self.token = self.token.trim().to_string();
        self.topics = self.topics.trim().to_string();
        self.terminal_alias = self.terminal_alias.trim().to_string();
        self.terminal_labels = self.terminal_labels.trim().to_string();
        self.terminal_id = self.terminal_id.trim().to_string();
        // 认不出来的值一律回落到气泡：老配置文件里没这个字段时是空串，
        // 手改错成 "topBar " 之类也不该让通知彻底不显示。
        self.display_mode = match self.display_mode.trim().to_lowercase().as_str() {
            DISPLAY_TOPBAR => DISPLAY_TOPBAR.into(),
            _ => DISPLAY_BUBBLE.into(),
        };
        if self.protocol.trim().is_empty() {
            self.protocol = "1".into();
        }
        if self.topics.is_empty() {
            self.topics = DEFAULT_TOPICS.into();
        }
        // NaN 会被 clamp 保留成 NaN，再写进 json 就是 null —— 显式挡掉
        self.sound_volume = if self.sound_volume.is_finite() {
            self.sound_volume.clamp(0.0, 1.0)
        } else {
            0.7
        };
        // 端点列表：去空白；url 为空的条目没有存在意义，直接丢
        self.endpoints.retain_mut(|e| {
            e.id = e.id.trim().to_string();
            e.label = e.label.trim().to_string();
            e.url = e.url.trim().to_string();
            e.token = e.token.trim().to_string();
            e.topics = e.topics.trim().to_string();
            !e.id.is_empty() && !e.url.is_empty()
        });
        // active 指向的端点被删光后，指针不能悬空
        if !self.active_endpoint.is_empty()
            && !self.endpoints.iter().any(|e| e.id == self.active_endpoint)
        {
            self.active_endpoint.clear();
        }
        self
    }

    /// 拆成订阅帧要的数组。顺手去掉空项和重复项 ——
    /// 用户手填 "a,,b, a" 这种不该变成三路订阅。
    pub fn topic_list(&self) -> Vec<String> {
        split_csv(&self.topics)
    }

    /// 标签同理。服务端按标签路由时，重复标签会让同一台机器被算进同一组两次。
    pub fn label_list(&self) -> Vec<String> {
        split_csv(&self.terminal_labels)
    }
}

fn split_csv(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',') {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// 内存里的权威副本。磁盘只是它的持久化，读的时候以内存为准 ——
/// 否则连点两次保存会读到上一次的旧值。
pub struct SettingsState(pub Mutex<AppSettings>);

pub fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> AppSettings {
    let Some(p) = path(app) else {
        return AppSettings::default();
    };
    std::fs::read_to_string(p)
        .ok()
        // 记事本和 PowerShell 5.1 存 UTF-8 都会带 BOM，而 serde_json 见到 BOM
        // 直接判定语法错。解析失败会静默回落默认值 —— 连「已静音」都会跟着丢，
        // 用户第二天醒来发现声音又自己开了。所以先削掉 BOM 再解析。
        .and_then(|raw| {
            serde_json::from_str::<AppSettings>(raw.trim_start_matches('\u{feff}')).ok()
        })
        .unwrap_or_default()
        .normalized()
}

pub fn save(app: &AppHandle, s: &AppSettings) -> Result<(), String> {
    let p = path(app).ok_or("拿不到应用数据目录")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(s).map_err(|e| format!("序列化设置失败: {e}"))?;
    std::fs::write(&p, raw).map_err(|e| format!("写入设置失败: {e}"))
}

/// 当前设置快照。连接管理和 mock 循环都从这里读。
pub fn current(app: &AppHandle) -> AppSettings {
    app.try_state::<SettingsState>()
        .map(|s| s.0.lock().map(|g| g.clone()).unwrap_or_default())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn settings_get(app: AppHandle) -> AppSettings {
    current(&app)
}

/// 整包覆盖保存。调用方是设置面板，它手上永远有完整的一份。
#[tauri::command]
pub fn settings_set(app: AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    let next = settings.normalized();
    if let Some(state) = app.try_state::<SettingsState>() {
        if let Ok(mut g) = state.0.lock() {
            *g = next.clone();
        }
    }
    match save(&app, &next) {
        Ok(()) => {
            // 必须广播给所有窗口：提示音在悬浮窗进程里响，它读的是自己
            // 进程内的 settings 副本。不广播的话「面板里静音了、气泡还在响」，
            // 用户会觉得静音开关是坏的。
            let _ = app.emit("ec:settings-updated", &next);
            Ok(next)
        }
        // 写盘失败也要把内存改掉：这一轮运行仍然按用户填的走，
        // 但必须把错误抛回去，不能让用户以为已经存住了。
        Err(e) => Err(format!("{e}（本次运行已生效，但重启后会丢失）")),
    }
}

/// 配置文件路径，设置面板上直接显示出来，方便手改和排障。
#[tauri::command]
pub fn settings_path(app: AppHandle) -> Option<String> {
    path(&app).map(|p| p.to_string_lossy().to_string())
}

/// 把服务端签发的 terminal_id 缓存下来，只用于展示。
///
/// 这是**缓存，不是身份**：真身份在服务端，客户端重新连一次就能拿回来。
/// 所以写盘失败只打日志不报错 —— 为一个展示字段弹错误框不值得。
pub fn cache_terminal_id(app: &AppHandle, id: &str) {
    if id.trim().is_empty() {
        return;
    }
    let next = {
        let Some(state) = app.try_state::<SettingsState>() else {
            return;
        };
        let Ok(mut g) = state.0.lock() else { return };
        if g.terminal_id == id {
            return;
        }
        g.terminal_id = id.to_string();
        g.clone()
    };
    if let Err(e) = save(app, &next) {
        eprintln!("[settings] terminal_id 缓存写盘失败（不影响连接）: {e}");
    }
}
