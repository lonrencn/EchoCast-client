//! 服务端 WebSocket 连接管理。
//!
//! 只做三件事：连上、把文本帧解析成 `TaskNotice` 推给前端、断了自动重连。
//! 协议细节全部收敛在 `parse_notice()` 里 —— 服务端换帧格式时只改这一个函数，
//! 连接循环和 UI 都不用动。
//!
//! 重连是无限指数退避（1s → 30s），没有「放弃」这个状态：
//! 桌面客户端的服务端可能晚几分钟起来，放弃重连只会让用户看到一片死寂。

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::mock::NOTICE_EVENT;
use crate::models::{TaskNotice, TaskStatus, TerminalInfo};
use crate::settings;

/// 连接状态事件名。前端订阅它来点亮状态灯。
pub const CONN_EVENT: &str = "ec:conn";

/// 服务端在 `hello_ack` 里下发的终端身份。前端拿它显示「我是谁」。
pub const TERMINAL_EVENT: &str = "ec:terminal";

/// 任务被撤回（抢单场景下别人先接了）。前端收到就收掉对应气泡。
pub const REVOKE_EVENT: &str = "ec:revoke";

/// 服务端对某次上行动作的应答（收下 / 拒收）。
///
/// 没有这一条，客户端只能把「写进发送队列」当成「对方收到了」——
/// 用户点了「今晚先停」，服务端把帧丢进 try/catch，界面上却是「已答复」。
/// 假成功比重跑一次训练贵得多。
pub const ACTION_RESULT_EVENT: &str = "ec:action-result";

/// 心跳间隔。WS 协议层的 Ping 帧，服务端必须回 Pong，不需要自定义业务心跳。
const HEARTBEAT: Duration = Duration::from_secs(25);
const BACKOFF_MIN: u64 = 1;
const BACKOFF_MAX: u64 = 30;

/// 连接状态。`retry_in` 让界面能说「8 秒后重试」而不是干等。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnStatus {
    /// 没配地址，或用户主动断开。
    Idle,
    Connecting {
        url: String,
    },
    Connected {
        url: String,
        since: i64,
    },
    Error {
        message: String,
        /// 距离下次重连还有几秒；0 表示立刻重试。
        retry_in: u64,
    },
    /// 配置层错误（token 无效、被吊销）。重连一万次也不会好，
    /// 所以必须停下来让用户改配置 —— 不停的话服务端日志会被 401 刷满。
    Fatal {
        message: String,
    },
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct ConnState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    status: Option<ConnStatus>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// 往当前连接写帧的通道。没连着时为 None，
    /// 这样「点动作但没连服务端」会立刻失败而不是静默排队积压。
    tx: Option<mpsc::UnboundedSender<String>>,
    /// 服务端在 hello_ack 里认下的身份。None 表示还没对过号。
    terminal: Option<TerminalInfo>,
    /// 已经回过 `seen` 的通知 id。
    ///
    /// 一条任务会推 started / running×N / completed 共五六帧，每帧都回一次 seen
    /// 只是噪音 —— 服务端关心的是「这条被看到了没有」，不是「看了几眼」。
    /// 上限存在是为了长时间运行不涨内存：超过就清空重来，重发一次 seen 无害。
    seen_ids: HashSet<String>,
    /// 最近一次推送的通知内容指纹（id -> FNV hash）。
    ///
    /// 服务端重连后会重放未确认的队列，有的实现还会循环重发 ——
    /// 不拦的话同一帧每来一次气泡就重新弹一遍，服务端一停数据、
    /// 桌面上反而像坏了一样循环出通知。进度帧内容必然变化，不会被误杀。
    emitted: std::collections::HashMap<String, u64>,
    /// 调试环形缓冲：最近 200 帧原始收发记录（ts_ms, 方向, 内容）。
    ///
    /// 常开、纯内存。「服务端到底发了什么」这个问题只在事后问得出来，
    /// 事后才开开关来不及。一条帧一次 push_back，正常使用付得起。
    debug: std::collections::VecDeque<(i64, String, String)>,
}

impl Default for ConnState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }
}

fn emit_status(app: &AppHandle, status: ConnStatus) {
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            g.status = Some(status.clone());
        }
    }
    // 连接状态的变化也进调试流：排障时「发了帧但没连上」和「没发帧」要分得开
    debug_push(app, "SYS", &format!("{status:?}"));
    if let Err(e) = app.emit(CONN_EVENT, &status) {
        eprintln!("[conn] 状态推送失败: {e}");
    }
}

/// 调试环形缓冲上限。
const DEBUG_CAP: usize = 200;

/// 直接往调试缓冲塞一条（不走 wslog 的文件日志分支）。
fn debug_push(app: &AppHandle, dir: &str, text: &str) {
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            g.debug.push_back((ms, dir.to_string(), text.chars().take(2000).collect()));
            while g.debug.len() > DEBUG_CAP {
                g.debug.pop_front();
            }
        }
    }
}

/// 调试面板拉取的帧记录。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DebugFrame {
    pub ts: i64,
    pub dir: String,
    pub text: String,
}

/// 最近 200 帧原始收发记录，新帧在前。设置页的隐藏调试入口用。
#[tauri::command]
pub fn debug_frames(app: AppHandle) -> Vec<DebugFrame> {
    let Some(state) = app.try_state::<ConnState>() else {
        return Vec::new();
    };
    let Ok(g) = state.inner.lock() else {
        return Vec::new();
    };
    g.debug
        .iter()
        .rev()
        .map(|(ts, dir, text)| DebugFrame {
            ts: *ts,
            dir: dir.clone(),
            text: text.clone(),
        })
        .collect()
}

/// 清空调试缓冲。
#[tauri::command]
pub fn debug_clear(app: AppHandle) {
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            g.debug.clear();
        }
    }
}

/// 通知内容指纹。同 id 但内容有变（状态、进度、问询、结果）时哈希必然不同，
/// 所以只有「完全相同的重放帧」会被拦住。
fn notice_hash(n: &TaskNotice) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    if let Ok(bytes) = serde_json::to_vec(n) {
        for b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 这条通知是不是该推给前端。完全重复的重放帧返回 false。
fn should_emit(app: &AppHandle, n: &TaskNotice) -> bool {
    let Some(state) = app.try_state::<ConnState>() else {
        return true;
    };
    let h = notice_hash(n);
    let Ok(mut g) = state.inner.lock() else {
        return true;
    };
    if g.emitted.get(&n.id) == Some(&(h)) {
        return false;
    }
    if g.emitted.len() > 512 {
        g.emitted.clear();
    }
    g.emitted.insert(n.id.clone(), h);
    true
}

// ---------------------------------------------------------------------------
// 地址
// ---------------------------------------------------------------------------

/// 把用户填的东西收敛成一个能直接连的地址。
///
/// 允许三种输入：`ws://…`、`wss://…`、以及裸的 `host:port`
/// （裸地址一律补 `ws://`）。填 `http://` 是常见手误，顺手改成 `ws://`
/// 比报错更省事 —— 这两者本来就指向同一个端口。
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("先填服务端地址".into());
    }
    let out = if let Some(rest) = s.strip_prefix("wss://") {
        format!("wss://{rest}")
    } else if let Some(rest) = s.strip_prefix("ws://") {
        format!("ws://{rest}")
    } else if let Some(rest) = s.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = s.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{s}")
    };
    // 只剩协议头说明根本没填主机名
    let body = out
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    if body.is_empty() {
        return Err("地址缺少主机名".into());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 帧解析
// ---------------------------------------------------------------------------

/// 三种输入都认：
/// 1. 裸的严格通知：`{"id": "…", "task": "…", "status": "…"}`
/// 2. 包了一层的信封：`{"op": "notice", "notice": {…}}`
/// 3. 宽松结构：`{"topic": "…", "text": "…", "card": {…}}`
///    —— 服务端还没补齐 id/status/timestamp 时，由 `wire::from_value` 兜底。
///
/// ⚠️ 必须只走 `wire::from_value` 一条路。曾经有「先试严格反序列化」的
/// 快路径：字段齐全的帧直接落成 TaskNotice，serde 把结构体不认识的字段
/// （比如 note_alias 这类别名）**静默丢弃**——同一条帧在两条路径下行为
/// 不一致，字段越加越多必然再踩。宽松归一是唯一真相源。
pub fn parse_notice(text: &str) -> Option<TaskNotice> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    crate::wire::from_value(v)
}

/// 认下服务端给的身份。
///
/// 这是路由闭环的起点：服务端说「你是 term_7f3a」，之后所有发给这个 id 的任务
/// 才会落到这条连接。客户端不参与这个判断，只是把结果记下来并显示给用户 ——
/// 用户至少要能确认「这台机器被认成了哪一台」，否则多机场景下会一头雾水。
fn on_hello_ack(app: &AppHandle, v: &serde_json::Value) {
    let Some(t) = v.get("terminal").filter(|t| t.is_object()) else {
        return;
    };
    let Some(id) = t.get("id").and_then(|i| i.as_str()).filter(|s| !s.is_empty()) else {
        return;
    };

    let info = TerminalInfo {
        id: id.to_string(),
        alias: t
            .get("alias")
            .and_then(|a| a.as_str())
            .unwrap_or_default()
            .to_string(),
        labels: t
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    };

    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            g.terminal = Some(info.clone());
        }
    }
    settings::cache_terminal_id(app, &info.id);
    let _ = app.emit(TERMINAL_EVENT, &info);
}

/// `task.deliver`：服务端指定投给本机的任务。
///
/// 转换时**必须把状态压成 running**：`wire::from_value` 在缺 status 时会兜底成
/// `success`（那是为通知设计的），投递过来的任务显然还没完成，
/// 照搬会画出「已完成」的卡片，用户点进去发现什么都没有。
fn on_task_deliver(app: &AppHandle, v: &serde_json::Value) -> Option<TaskNotice> {
    let task = v.get("task").filter(|t| t.is_object())?;
    let mut n = crate::wire::from_value(task.clone())?;

    if task.get("status").is_none() {
        n.status = TaskStatus::Running;
        n.progress = None;
        if n.result.trim().is_empty() {
            n.result = "等待处理".into();
        }
    }

    // 投递即视为「已送达」。这个回执必须自动发，不能等用户点按钮 ——
    // 服务端靠它区分「发了没人看」和「看了没处理」，后者才是要催的。
    mark_seen(app, &n.id);

    Some(n)
}

fn on_task_revoke(app: &AppHandle, v: &serde_json::Value) {
    let id = v
        .get("task_id")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())
        .unwrap_or_default();
    if id.is_empty() {
        return;
    }
    #[derive(serde::Serialize, Clone)]
    struct Revoke<'a> {
        id: &'a str,
        reason: &'a str,
    }
    let _ = app.emit(
        REVOKE_EVENT,
        Revoke {
            id,
            reason: v.get("reason").and_then(|r| r.as_str()).unwrap_or("已由其他终端处理"),
        },
    );
}

/// 解析出的一帧动作回执。
#[derive(Debug, PartialEq)]
pub struct ActionOutcome {
    pub id: String,
    pub action_id: String,
    pub ok: bool,
    pub reason: Option<String>,
}

/// 从一帧任意形状的回执里拆出「收下了没有」。
///
/// 判 ok 这件事**不能靠 op 名一个字段说了算**：真实服务端第一版回的是
/// `{"op":"action_nack","reason":"unknown_notice"}`，没有 `ok` 字段；
/// 也有服务端就发 `{"notice_id":…,"ok":false}` 不带 op。
/// 所以顺序是：op 带 nack/error 判否 → 否则看布尔字段 → 都没有就当成功。
pub fn parse_action_result(v: &serde_json::Value) -> Option<ActionOutcome> {
    let id = ["notice_id", "task_id", "id"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .filter(|s| !s.trim().is_empty())?;

    let op = v
        .get("op")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_lowercase();
    let ok = if op.ends_with("nack") || op.ends_with("error") || op.ends_with("fail") {
        false
    } else {
        ["ok", "success", "accepted"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_bool()))
            // 有 op 且不是 nack，本身就是「收到了」的声明；两样都没有才兜底成功。
            .unwrap_or(true)
    };

    let reason = ["reason", "message", "error", "detail"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Some(ActionOutcome {
        id: id.to_string(),
        action_id: ["action_id", "option_id"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
            .unwrap_or_default()
            .to_string(),
        ok,
        reason,
    })
}

/// 服务端对某次上行动作的应答。
///
/// 认 `action_ack` / `action_nack` / `action.result` 这类 op（服务端命名还没定死，
/// 见 `tools/bridge-msg8.md`），也认没有 op 只有 `ok` 的裸回执 —— 宁可多认一种，
/// 也别让用户以为答上了、其实被丢了。
fn on_action_result(app: &AppHandle, v: &serde_json::Value) {
    let Some(out) = parse_action_result(v) else {
        return;
    };

    #[derive(serde::Serialize, Clone)]
    struct Outcome {
        id: String,
        action_id: String,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    }

    let _ = app.emit(
        ACTION_RESULT_EVENT,
        Outcome {
            id: out.id,
            action_id: out.action_id,
            ok: out.ok,
            reason: out.reason,
        },
    );
}

fn dispatch(app: &AppHandle, text: &str) {
    // 控制帧优先按 op 分流。它们一定带 op，而通知可能带也可能不带，
    // 所以「先看 op」不会误伤通知；反过来先按通知解析会把 hello_ack 猜成一条空气泡。
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(op) = v.get("op").and_then(|o| o.as_str()) {
            match op {
                "hello_ack" => {
                    on_hello_ack(app, &v);
                    return;
                }
                "task.deliver" => {
                    match on_task_deliver(app, &v) {
                        Some(n) if should_emit(app, &n) => {
                            if let Some(media) = &n.media {
                                crate::media::prefetch(app, media, Some(settings::current(app).token));
                            }
                            if let Err(e) = app.emit(NOTICE_EVENT, &n) {
                                eprintln!("[conn] 任务投递推送失败: {e}");
                            }
                        }
                        // 重复重放帧：seen 已回过，这里静默丢掉即可
                        Some(_) => {}
                        None => eprintln!("[conn] task.deliver 结构不可用，已丢弃"),
                    }
                    return;
                }
                "task.revoke" => {
                    on_task_revoke(app, &v);
                    return;
                }
                // 回执：服务端对我们发出的动作/答复表态。
                // 前缀匹配而不是逐个枚举 —— 命名还没和对面定死，
                // 少认一种，用户就会看到一个假的「已答复」。
                o if o != "action" && o.starts_with("action") => {
                    on_action_result(app, &v);
                    return;
                }
                // 心跳、订阅确认、别台终端的在线状态：都不是通知，直接吃掉。
                // 少了这一支，它们会掉进下面的宽松解析里被拼成气泡。
                "pong" | "subscribed" | "unsubscribed" | "presence" | "ping" => return,
                _ => {}
            }
        }
    }

    match parse_notice(text) {
        Some(n) => {
            // 广播通知也要回 seen。以前只有 `task.deliver` 回，
            // 而服务端推的全是广播，所以「这台终端看到这条了」从没上报过。
            mark_seen(app, &n.id);
            // 服务端重连重放 / 循环重发时，同一帧不该让气泡重新弹一遍
            if !should_emit(app, &n) {
                return;
            }
            // 带附件的通知顺手预取：点开面板时文件应该已经在本地了。
            // 失败静默 —— 面板里有重试，后台失败不该变成第二条通知。
            if let Some(media) = &n.media {
                crate::media::prefetch(app, media, Some(settings::current(app).token));
            }
            if let Err(e) = app.emit(NOTICE_EVENT, &n) {
                eprintln!("[conn] 通知推送失败: {e}");
            }
        }
        None => {
            // 心跳、订阅确认这类非通知帧会走到这里，不该当成错误刷屏，
            // 但排障时需要原样看到它，所以只在 debug 构建里打。
            #[cfg(debug_assertions)]
            eprintln!("[conn] 丢弃非通知帧: {}", text.chars().take(160).collect::<String>());
        }
    }
}

// ---------------------------------------------------------------------------
// 连接循环
// ---------------------------------------------------------------------------

/// 一次连接的结束方式。区分「能重连」和「重连也没用」是这套路由方案的前提：
/// token 填错时无限重连，服务端日志会被 401 刷满，真问题反而看不见。
enum PumpEnd {
    /// 对端正常关闭。
    Closed,
    /// 网络或服务端临时故障，退避后重连。
    Retry(String),
    /// 配置层错误，停止重连，等用户改设置。
    Fatal(String),
}

/// 服务端用 close code 表达「别再来了」。这几个码必须认，
/// 光靠错误文本判断会被不同的库文案坑。
fn fatal_close(code: u16) -> Option<String> {
    match code {
        4001 => Some("token 无效或已过期，请在设置里重新填写".into()),
        4003 => Some("该终端已被管理员吊销，请联系服务端".into()),
        4029 => Some("终端已被其他实例占用".into()),
        _ => None,
    }
}

/// 帧日志。只在 `ECHOCAST_WS_LOG=1` 时写文件。
///
/// 存在的理由很具体：服务端「不推帧」和「推了但格式不对」在界面上长得一模一样 ——
/// 都是没有气泡。打开它就有一份双方逐帧的原始记录，不用靠猜。
/// 默认关闭：这条路径每收一条通知就开一次文件，正常使用不该付这个代价。
fn wslog(app: &AppHandle, dir: &str, text: &str) {
    // 环形缓冲常开（调试面板的 data 来源）；文件日志仍然只在
    // ECHOCAST_WS_LOG=1 时写 —— 后者要付磁盘 IO，前者只是内存 push。
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            g.debug.push_back((
                ms,
                dir.to_string(),
                text.chars().take(2000).collect(),
            ));
            while g.debug.len() > DEBUG_CAP {
                g.debug.pop_front();
            }
        }
    }

    if std::env::var("ECHOCAST_WS_LOG").is_err() {
        return;
    }
    let Ok(p) = app.path().app_data_dir().map(|d| d.join("ws-frames.log")) else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "{ms} {dir} {text}");
    }
}

async fn pump(
    app: &AppHandle,
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    mut rx: mpsc::UnboundedReceiver<String>,
) -> PumpEnd {
    let (mut sink, mut stream) = ws.split();
    let mut hb = tokio::time::interval(HEARTBEAT);
    hb.tick().await; // interval 的第一次 tick 是立即返回的，先吃掉

    loop {
        tokio::select! {
            // 前端点动作 → 走这条上行。放在同一条连接里，
            // 不用为回传再开一趟 HTTP。
            outbound = rx.recv() => {
                match outbound {
                    Some(text) => {
                        wslog(app, "→", &text);
                        if sink.send(Message::Text(text.into())).await.is_err() {
                            return PumpEnd::Retry("上行发送失败".into());
                        }
                    }
                    // 通道两端都断了，说明连接正在被拆掉
                    None => return PumpEnd::Closed,
                }
            }
            _ = hb.tick() => {
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return PumpEnd::Retry("心跳发送失败".into());
                }
            }
            incoming = stream.next() => {
                let Some(msg) = incoming else { return PumpEnd::Closed };
                match msg {
                    Ok(Message::Text(t)) => {
                        wslog(app, "←", t.as_str());
                        dispatch(app, t.as_str());
                    }
                    Ok(Message::Ping(p)) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(f) = frame {
                            let code = u16::from(f.code);
                            if let Some(msg) = fatal_close(code) {
                                return PumpEnd::Fatal(msg);
                            }
                            if code != 1000 && code != 1005 {
                                return PumpEnd::Retry(format!("服务端关闭连接（{code}）"));
                            }
                        }
                        return PumpEnd::Closed;
                    }
                    Ok(_) => {}
                    Err(e) => return PumpEnd::Retry(format!("读取失败: {e}")),
                }
            }
        }
    }
}

/// 握手请求。服务端在 connection 事件里比对 token_hash，
/// 所以 token 必须放在 Upgrade 的请求头里，不能靠首帧再补。
fn handshake(url: &str, token: &str) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let mut req = url
        .to_string()
        .into_client_request()
        .map_err(|e| format!("地址不合法: {e}"))?;
    let token = token.trim();
    if !token.is_empty() {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "token 里有非法字符（换行或全角空格？）".to_string())?;
        req.headers_mut().insert("Authorization", value);
    }
    Ok(req)
}

/// 连接建立后立刻订阅。服务端的 topic 不做通配，
/// 少发一个 topic 就是彻底收不到那一路通知。
fn subscribe_frame(app: &AppHandle) -> String {
    let topics = settings::current(app).topic_list();
    serde_json::json!({ "op": "subscribe", "topics": topics }).to_string()
}

/// 自报身份。必须在订阅之前发：服务端要靠它把「这条连接」登记成一台终端，
/// 否则第一轮任务找不到投递目标。
///
/// 注意这里报的是**建议值**（别名、标签、设备信息），真正生效的 terminal_id
/// 由服务端在 hello_ack 里下发 —— 客户端自报的 id 一文不值。
fn hello_frame(app: &AppHandle) -> String {
    let s = settings::current(app);
    let dev = crate::device::load_or_create(app);

    let alias = if s.terminal_alias.trim().is_empty() {
        dev.hostname.clone()
    } else {
        s.terminal_alias.clone()
    };

    serde_json::json!({
        "op": "hello",
        "protocol": s.protocol,
        "alias": alias,
        "labels": s.label_list(),
        "device": {
            "device_id": dev.device_id,
            "hostname": dev.hostname,
            "os": dev.os,
            "app_version": dev.app_version,
        }
    })
    .to_string()
}

fn ack_frame(task_id: &str, state: &str) -> String {
    serde_json::json!({ "op": "ack", "task_id": task_id, "state": state }).to_string()
}

/// 往当前连接写一帧。没连着就静默丢弃 ——
/// 调用方都是「连接已建立」之后才会走到这里。
fn send_frame(app: &AppHandle, text: &str) {
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(g) = state.inner.lock() {
            if let Some(tx) = g.tx.as_ref() {
                let _ = tx.send(text.to_string());
            }
        }
    }
}

/// 回一次 `seen`，同一条通知只回一次。
///
/// 服务端靠它区分「发了没人看」和「看了没处理」，后者才是要催的。
///
/// 这个回执以前只挂在 `task.deliver` 上，而服务端现在推的全是广播
/// `op:"notice"` —— 等于「已读」这类交互内容一条都没上过行。
fn mark_seen(app: &AppHandle, id: &str) {
    if id.is_empty() {
        return;
    }
    // 锁必须先放掉再 send_frame：send_frame 内部要再拿同一把锁，不释放就自锁。
    let first = {
        let Some(state) = app.try_state::<ConnState>() else {
            return;
        };
        let Ok(mut g) = state.inner.lock() else {
            return;
        };
        if g.seen_ids.len() > 512 {
            g.seen_ids.clear();
        }
        g.seen_ids.insert(id.to_string())
    };
    if first {
        send_frame(app, &ack_frame(id, "seen"));
    }
}

async fn connect_loop(app: AppHandle, url: String) {
    let mut backoff = BACKOFF_MIN;

    loop {
        emit_status(
            &app,
            ConnStatus::Connecting {
                url: url.clone(),
            },
        );

        let token = settings::current(&app).token;
        let req = match handshake(&url, &token) {
            Ok(r) => r,
            Err(message) => {
                emit_status(&app, ConnStatus::Error { message, retry_in: backoff });
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };

        match tokio_tungstenite::connect_async(req).await {
            Ok((mut ws, _)) => {
                backoff = BACKOFF_MIN;
                emit_status(
                    &app,
                    ConnStatus::Connected {
                        url: url.clone(),
                        since: now_ms(),
                    },
                );

                // 先自报身份，再订阅。
                // 顺序反了的话，服务端收到 subscribe 时还不知道对面是哪台终端，
                // 第一轮按终端投递的任务会找不到目标。
                let hello = hello_frame(&app);
                wslog(&app, "→", &hello);
                if ws.send(Message::Text(hello.into())).await.is_err() {
                    emit_status(
                        &app,
                        ConnStatus::Error {
                            message: "身份帧发送失败".into(),
                            retry_in: backoff,
                        },
                    );
                }

                // 订阅必须在这里发，不能等 pump 起来 ——
                // pump 是读循环，订阅晚了这一段的通知就漏了。
                let sub = subscribe_frame(&app);
                wslog(&app, "→", &sub);
                if ws.send(Message::Text(sub.clone().into())).await.is_err() {
                    emit_status(
                        &app,
                        ConnStatus::Error {
                            message: "订阅帧发送失败".into(),
                            retry_in: backoff,
                        },
                    );
                }

                let (tx, rx) = mpsc::unbounded_channel::<String>();
                if let Some(state) = app.try_state::<ConnState>() {
                    if let Ok(mut g) = state.inner.lock() {
                        g.tx = Some(tx);
                    }
                }

                match pump(&app, ws, rx).await {
                    PumpEnd::Closed => {}
                    PumpEnd::Retry(message) => {
                        emit_status(
                            &app,
                            ConnStatus::Error {
                                message,
                                retry_in: backoff,
                            },
                        );
                    }
                    PumpEnd::Fatal(message) => {
                        // 配置错了。重连一万次也是同一个 401，
                        // 所以这里必须退出循环，把问题交回给用户。
                        if let Some(state) = app.try_state::<ConnState>() {
                            if let Ok(mut g) = state.inner.lock() {
                                g.tx = None;
                                g.terminal = None;
                            }
                        }
                        emit_status(&app, ConnStatus::Fatal { message });
                        return;
                    }
                }
            }
            Err(e) => {
                if let Some(message) = fatal_handshake(&e) {
                    if let Some(state) = app.try_state::<ConnState>() {
                        if let Ok(mut g) = state.inner.lock() {
                            g.terminal = None;
                        }
                    }
                    emit_status(&app, ConnStatus::Fatal { message });
                    return;
                }
                emit_status(
                    &app,
                    ConnStatus::Error {
                        message: friendly(&e),
                        retry_in: backoff,
                    },
                );
            }
        }

        // 连接已经断了，上行通道必须一起清掉，
        // 否则用户点动作会往一个死通道里写，还不报错。
        if let Some(state) = app.try_state::<ConnState>() {
            if let Ok(mut g) = state.inner.lock() {
                g.tx = None;
            }
        }

        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// 握手阶段就被拒，说明是 token 的问题，不是网络的问题。
/// 这两种要分开处理：网络问题要重连，token 问题要用户动手。
fn fatal_handshake(e: &tokio_tungstenite::tungstenite::Error) -> Option<String> {
    use tokio_tungstenite::tungstenite::Error;
    let Error::Http(resp) = e else { return None };
    match resp.status().as_u16() {
        401 => Some("服务端不认这个 token（401）：可能填错、已过期或已被吊销".into()),
        403 => Some("该终端没有接入权限（403）：请让服务端确认这台机器的授权".into()),
        _ => None,
    }
}

/// 库抛的错误对用户没意义，翻译成人话。
fn friendly(e: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match e {
        Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            "连接被拒绝：服务端没起，或端口不对".into()
        }
        Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => "连接超时".into(),
        Error::Http(resp) => match resp.status().as_u16() {
            401 | 403 => "服务端拒绝：需要鉴权".into(),
            404 => "这个路径上没有 WS 服务".into(),
            code => format!("握手失败 HTTP {code}"),
        },
        Error::Url(u) => format!("地址不合法: {u}"),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn conn_status(app: AppHandle) -> ConnStatus {
    app.try_state::<ConnState>()
        .and_then(|s| s.inner.lock().ok().and_then(|g| g.status.clone()))
        .unwrap_or(ConnStatus::Idle)
}

/// 连接（或换一个地址重连）。返回规范化后的地址，前端拿它回填输入框。
#[tauri::command]
pub fn conn_connect(app: AppHandle, url: String) -> Result<String, String> {
    let target = normalize_url(&url)?;
    start(&app, target.clone());
    Ok(target)
}

#[tauri::command]
pub fn conn_disconnect(app: AppHandle) {
    stop(&app);
}

/// 把用户点的动作回传给服务端。
///
/// 服务端定的上行形状是 `{op:"action", notice_id, action_id, payload}`，
/// 走同一条 WS 连接，不另开 HTTP。
///
/// `notice_id` 必须是我们收到的真实 `id`。服务端当前的通知里还没有这个字段，
/// 客户端归一化时会用「任务名 + 执行方」算一个指纹顶上 —— 但指纹只在客户端内部
/// 唯一，服务端认不出来，所以动作回传要真正闭环，服务端必须补 `id`。
#[tauri::command]
pub fn conn_send_action(
    app: AppHandle,
    notice_id: String,
    action_id: String,
    payload: Option<String>,
) -> Result<(), String> {
    let mut frame = serde_json::json!({
        "op": "action",
        "notice_id": notice_id,
        "action_id": action_id,
    });
    if let Some(p) = payload.filter(|p| !p.trim().is_empty()) {
        frame["payload"] = serde_json::Value::String(p);
    }
    let text = frame.to_string();
    send_or_err(&app, text)
}

/// 用户对任务表态。
///
/// `seen` 由客户端收到任务时自动回，这个命令给的是用户级回执：
/// `accepted`（接了）/ `rejected`（跳过）/ `done`（做完了）。
///
/// 服务端靠这四态判断投递是否闭环。只有 `seen` 的话，服务端只知道
/// 「发出去了」，不知道有没有人真的处理 —— 那和没发出去区别不大。
#[tauri::command]
pub fn conn_send_ack(
    app: AppHandle,
    task_id: String,
    state: String,
    reason: Option<String>,
) -> Result<(), String> {
    let state = state.trim();
    if !matches!(state, "seen" | "accepted" | "rejected" | "done") {
        return Err("ack 状态只能是 seen / accepted / rejected / done".into());
    }
    if task_id.trim().is_empty() {
        return Err("task_id 不能为空".into());
    }

    let mut frame = serde_json::json!({ "op": "ack", "task_id": task_id, "state": state });
    if let Some(r) = reason.filter(|r| !r.trim().is_empty()) {
        frame["reason"] = serde_json::Value::String(r);
    }
    send_or_err(&app, frame.to_string())
}

/// 当前连接被服务端认成的终端身份。没连上或服务端还没回 hello_ack 时为 None。
#[tauri::command]
pub fn terminal_get(app: AppHandle) -> Option<TerminalInfo> {
    app.try_state::<ConnState>()
        .and_then(|s| s.inner.lock().ok().and_then(|g| g.terminal.clone()))
}

fn send_or_err(app: &AppHandle, text: String) -> Result<(), String> {
    let state = app.try_state::<ConnState>().ok_or("连接状态不可用")?;
    let guard = state.inner.lock().map_err(|_| "连接状态被占用")?;
    let tx = guard.tx.as_ref().ok_or("还没连上服务端，这一帧没发出去")?;
    tx.send(text).map_err(|_| "连接已断开，这一帧没发出去".to_string())
}

/// 当前是否真的连着一个服务端。
///
/// mock 循环靠它决定要不要闭嘴：一旦连上真实服务端，假数据必须立刻停 ——
/// 两路同时往浮层推，用户分不清哪条是真的，排障时这种混淆比没数据更糟。
pub fn is_live(app: &AppHandle) -> bool {
    app.try_state::<ConnState>()
        .and_then(|s| {
            s.inner
                .lock()
                .ok()
                .map(|g| matches!(g.status, Some(ConnStatus::Connected { .. })))
        })
        .unwrap_or(false)
}

pub fn start(app: &AppHandle, url: String) {
    stop_quiet(app); // 换地址时先把旧任务掐掉，否则会有两条循环抢着写状态

    let task = tauri::async_runtime::spawn(connect_loop(app.clone(), url));
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            g.task = Some(task);
        }
    }
}

/// 启动后延迟连接。等前端把 `ec:conn` 订阅建好，否则第一个状态就丢了 ——
/// 界面会一直停在「未连接」，直到下一次重连才亮起来。
pub fn start_delayed(app: AppHandle, url: String, delay: Duration) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        if let Ok(target) = normalize_url(&url) {
            start(&app, target);
        }
    });
}

fn stop_quiet(app: &AppHandle) {
    if let Some(state) = app.try_state::<ConnState>() {
        if let Ok(mut g) = state.inner.lock() {
            if let Some(h) = g.task.take() {
                h.abort();
            }
            // 上行通道跟着断。留着它的话，断开后点动作会写进一个没人读的队列，
            // 前端拿到 Ok 却什么也没发出去 —— 这种假成功最难查。
            g.tx = None;
            // 身份也一并作废：断开之后「我连上了哪台终端」就不再成立，
            // 界面继续显示旧 terminal 会让人误以为还连着。
            g.terminal = None;
        }
    }
}

pub fn stop(app: &AppHandle) {
    // 主动断开不能触发重连，所以必须先 abort 再改状态 ——
    // 顺序反了的话，循环退出前会再写一次 Connecting 回来。
    stop_quiet(app);
    emit_status(app, ConnStatus::Idle);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn notice_envelope_with_alias_field_survives_parsing() {
        // 回归：字段齐全的帧曾经走严格反序列化，serde 把结构体不认识的
        // note_alias 静默丢掉，别名字段只有单测里（wire 层）能用。
        // parse_notice 必须整条走宽松归一。
        let n = parse_notice(
            r#"{"op":"notice","notice":{
                "id":"t-alias","task":"带别名的任务","status":"success",
                "executor":{"name":"Research Agent"},
                "result":"ok","actions":[],"timestamp":1790000000000,
                "note_alias":"研究组-小陈"}}"#,
        )
        .expect("完整帧必须解析成功");
        assert_eq!(n.agent_name.as_deref(), Some("研究组-小陈"));
        assert_eq!(n.executor.name, "Research Agent");
    }

    #[test]
    fn nack_without_ok_field_still_means_no() {
        // 真实服务端第一版回的就是这个：只有 op 和 reason，没有 ok。
        // 当成成功的话，界面会显示一个漂亮的绿色「已答复」，而什么都没发生。
        let out = parse_action_result(&v(
            r#"{"op":"action_nack","notice_id":"n1","action_id":"hold","reason":"unknown_notice"}"#,
        ))
        .unwrap();
        assert_eq!(out.id, "n1");
        assert_eq!(out.action_id, "hold");
        assert!(!out.ok);
        assert_eq!(out.reason.as_deref(), Some("unknown_notice"));
    }

    #[test]
    fn ack_is_ok_even_without_flag() {
        let out =
            parse_action_result(&v(r#"{"op":"action_ack","notice_id":"n1","action_id":"hold"}"#))
                .unwrap();
        assert!(out.ok);
        assert!(out.reason.is_none());
    }

    #[test]
    fn bare_receipt_with_ok_false() {
        // 没有 op，只有 ok —— 别的服务端很可能这么写
        let out = parse_action_result(&v(r#"{"notice_id":"n2","ok":false,"message":"过期了"}"#))
            .unwrap();
        assert!(!out.ok);
        assert_eq!(out.reason.as_deref(), Some("过期了"));
    }

    #[test]
    fn receipt_without_notice_id_is_ignored() {
        // 对不上是哪条通知，改不了任何界面状态，直接丢
        assert!(parse_action_result(&v(r#"{"op":"action_ack","action_id":"hold"}"#)).is_none());
    }

    #[test]
    fn auth_close_codes_stop_retrying() {
        // 这三种是配置错，重连一万次也是同样的结果
        assert!(fatal_close(4001).is_some(), "token 无效必须停止重连");
        assert!(fatal_close(4003).is_some(), "被吊销必须停止重连");
        assert!(fatal_close(4029).is_some(), "被占用要提示，但也不必无限重试");
        // 正常关闭和异常断开是网络层的事，应该继续重连
        assert!(fatal_close(1000).is_none());
        assert!(fatal_close(1006).is_none());
    }

    #[test]
    fn handshake_401_is_fatal_but_404_is_not() {
        use tokio_tungstenite::tungstenite::http::{Response, StatusCode};
        use tokio_tungstenite::tungstenite::Error;

        let unauthorized = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(None::<Vec<u8>>)
            .unwrap();
        assert!(
            fatal_handshake(&Error::Http(Box::new(unauthorized))).is_some(),
            "401 是 token 问题，要用户改配置"
        );

        // 404 说明地址写错了路径，但服务端可能是刚重启还没注册路由 ——
        // 归到普通错误里让它重试，比直接判定配置错误更不容易误伤。
        let not_found = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(None::<Vec<u8>>)
            .unwrap();
        assert!(fatal_handshake(&Error::Http(Box::new(not_found))).is_none());
    }
}
