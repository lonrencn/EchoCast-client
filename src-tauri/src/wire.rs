// 线路帧的「宽松解析」层。
//
// 为什么需要这一层：服务端和服务端的 agent 很可能先按 `{topic, text, card}`
// 这种偏展示的结构推，而不是按 `TaskNotice` 的严格形状。等对方改要时间，
// 而客户端完全有能力自己补齐 —— 缺 id 就按内容算一个稳定指纹，缺 status
// 就从 topic 推，缺 timestamp 就取当前时间。
//
// 定位：这是**兼容层，不是契约**。契约仍然是 docs/notice-protocol.md，
// 归一化只是让联调期少一轮来回。服务端补齐字段后，这一层自然走不到。
//
// 硬约束：认不出的帧必须返回 None，绝不能凭空造出一条通知 ——
// 服务端发的 `{op:"pong"}` 变成气泡比丢帧难排查得多。

use serde::Deserialize;

use crate::models::{
    ActionStyle, AskOption, Executor, ExecutorKind, MediaItem, MediaKind, NoticeAction, NoticeAsk,
    TaskNotice, TaskStatus,
};

/// 一条通知的宽松输入。所有字段都可选，缺什么补什么。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawNotice {
    pub id: Option<String>,
    pub task: Option<String>,
    /// `task.deliver` 用的字段名是 `title`，跟通知里的 `task` 是同一个东西。
    pub title: Option<String>,
    pub text: Option<String>,
    pub topic: Option<String>,
    pub status: Option<String>,
    pub result: Option<String>,
    pub detail: Option<String>,
    pub progress: Option<f64>,
    pub timestamp: Option<i64>,
    pub channel: Option<String>,
    pub buddy_pack: Option<String>,
    pub sticky: Option<bool>,
    /// 任务被路由到哪台终端（服务端标记，客户端只展示）。
    pub terminal: Option<String>,
    /// 离线补投时的原始创建时间。
    pub offline_since: Option<i64>,
    #[serde(default)]
    pub executor: Option<ExecIn>,
    #[serde(default)]
    pub actions: Option<Vec<RawAction>>,
    /// 问询：可以是 `ask:{question,options}`，也可以平铺成 `question` + `options`。
    pub ask: Option<RawAsk>,
    pub question: Option<String>,
    pub options: Option<Vec<RawAction>>,
    pub card: Option<RawCard>,
    /// 多媒体附件。字段名也宽松收：`media` / `attachments` / `files` 都认。
    pub media: Option<Vec<RawMedia>>,
    pub attachments: Option<Vec<RawMedia>>,
    pub files: Option<Vec<RawMedia>>,
    /// agent 属名（署名）。`agent` / `agent_name` / `agent_alias` 都收。
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub agent_alias: Option<String>,
    /// BROADCAST v1 信封的结构化载荷：`data.url` 是机器可读链接字段
    /// （2026-11-01 与服务端对齐：text 裸 URL 给人看，data.url 给程序读）。
    pub data: Option<RawData>,
    /// 顶层直接带 url 的写法也收，处理逻辑与 data.url 相同。
    pub url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawData {
    pub url: Option<String>,
}

/// 媒体附件的宽松输入。url 的字段名各服务端写法不一，一次收齐。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawMedia {
    pub kind: Option<String>,
    pub r#type: Option<String>,
    pub media_type: Option<String>,
    pub url: Option<String>,
    pub src: Option<String>,
    pub href: Option<String>,
    pub name: Option<String>,
    pub filename: Option<String>,
    pub title: Option<String>,
    pub mime: Option<String>,
    pub content_type: Option<String>,
    /// 服务端可能给浮点字节数，用 f64 收再取整，别让 2048.0 炸掉整条。
    pub size: Option<f64>,
    pub poster: Option<String>,
    pub thumb: Option<String>,
    pub thumbnail: Option<String>,
}

/// 问询的宽松输入。字段名各服务端写法不一，这里一次收齐。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawAsk {
    pub question: Option<String>,
    /// 有的服务端把问题写在 `text` 里，跟通知正文抢同一个字段名。
    pub text: Option<String>,
    pub prompt: Option<String>,
    pub options: Option<Vec<RawAction>>,
    pub choices: Option<Vec<RawAction>>,
    pub deadline: Option<i64>,
    pub context: Option<String>,
}

/// `executor` 可能是对象，也可能被写成裸字符串（"Research Agent"）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ExecIn {
    Name(String),
    Full(RawExecutor),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawExecutor {
    pub name: Option<String>,
    pub node: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawAction {
    pub id: Option<String>,
    pub label: Option<String>,
    pub style: Option<String>,
    pub payload: Option<String>,
    /// 服务端常见的写法是直接给 url，没有 payload 这个抽象。
    pub url: Option<String>,
}

/// 服务端 2e3e4da 那版卡片结构。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawCard {
    pub title: Option<String>,
    pub source: Option<String>,
    pub text: Option<String>,
    pub fields: Vec<RawField>,
    pub action: Option<RawAction>,
    pub actions: Option<Vec<RawAction>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawField {
    pub label: Option<String>,
    pub value: Option<String>,
}

/// 「任务」这类字段的 label 各服务端写法不一，统一按小写包含匹配。
const TASK_LABELS: &[&str] = &["任务", "任务名", "标题", "task", "title", "name"];
const RESULT_LABELS: &[&str] = &["结果", "结果摘要", "结论", "result", "summary", "output"];

fn field_of(card: &RawCard, labels: &[&str]) -> Option<String> {
    card.fields.iter().find_map(|f| {
        let label = f.label.as_deref()?.trim().to_lowercase();
        if labels.iter().any(|l| label.contains(l)) {
            f.value.as_ref().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
        } else {
            None
        }
    })
}

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// 本机绝对路径的样子：`C:\…`、`C:/…`、`\\server\share`、`/home/…`。
/// `data.url` 里给了路径时也要能开（sys_open 侧再做存在性与扩展名把关）。
fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    let mut it = s.chars();
    if let (Some(c), Some(':')) = (it.next(), it.next()) {
        if c.is_ascii_alphabetic() {
            return true;
        }
    }
    s.starts_with('\\') || s.starts_with('/')
}

/// topic → status。服务端的 topic 命名是它自己的，这里只做包含式匹配，
/// 不要求它改成我们的枚举。
fn status_from_topic(topic: &str) -> Option<TaskStatus> {
    let t = topic.to_lowercase();
    if t.contains("fail") || t.contains("error") || t.contains("abort") {
        return Some(TaskStatus::Failed);
    }
    if t.contains("warn") || t.contains("partial") {
        return Some(TaskStatus::Warning);
    }
    if t.contains("complet") || t.contains("success") || t.contains("done") || t.contains("finish") {
        return Some(TaskStatus::Success);
    }
    if t.contains("start") || t.contains("run") || t.contains("progress") || t.contains("tick") {
        return Some(TaskStatus::Running);
    }
    None
}

fn parse_status(s: &str) -> Option<TaskStatus> {
    match s.trim().to_lowercase().as_str() {
        "running" | "run" | "pending" | "started" | "progress" => Some(TaskStatus::Running),
        "success" | "succeeded" | "ok" | "done" | "completed" | "complete" => Some(TaskStatus::Success),
        "warning" | "warn" | "partial" => Some(TaskStatus::Warning),
        "failed" | "fail" | "error" | "aborted" => Some(TaskStatus::Failed),
        _ => None,
    }
}

/// FNV-1a。只在服务端没给 id 时用，目的是「同样的内容算同一个 id」，
/// 这样重复推进度会原地覆盖而不是冒出一排气泡。
fn fingerprint(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("auto-{h:016x}")
}

fn to_executor(v: ExecIn) -> Executor {
    match v {
        ExecIn::Name(n) => Executor { name: n, node: None, kind: ExecutorKind::Agent },
        ExecIn::Full(f) => Executor {
            name: f.name.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "Agent Center".into()),
            node: f.node.filter(|s| !s.trim().is_empty()),
            kind: match f.kind.as_deref().map(str::to_lowercase).as_deref() {
                Some("agent") => ExecutorKind::Agent,
                Some("remote") => ExecutorKind::Remote,
                _ => ExecutorKind::Local,
            },
        },
    }
}

/// 选项和动作是同一个形状，唯一区别是没给 id 时不要乱补一个 "open" ——
/// 问询的选项 id 是服务端用来判断「选了哪个」的，编一个就等于答非所问。
fn to_option(a: RawAction, index: usize) -> Option<AskOption> {
    let label = non_empty(&a.label)?;
    Some(AskOption {
        id: non_empty(&a.id).unwrap_or_else(|| format!("opt-{index}")),
        label,
        style: match a.style.as_deref().map(str::to_lowercase).as_deref() {
            Some("primary") => ActionStyle::Primary,
            _ => ActionStyle::Ghost,
        },
        payload: non_empty(&a.payload).or_else(|| non_empty(&a.url)),
    })
}

fn to_action(a: RawAction, fallback_id: &str) -> Option<NoticeAction> {
    let label = non_empty(&a.label)?;
    Some(NoticeAction {
        id: non_empty(&a.id).unwrap_or_else(|| fallback_id.to_string()),
        label,
        style: match a.style.as_deref().map(str::to_lowercase).as_deref() {
            Some("primary") => ActionStyle::Primary,
            _ => ActionStyle::Ghost,
        },
        payload: non_empty(&a.payload).or_else(|| non_empty(&a.url)),
    })
}

/// kind / mime / URL 扩展名 → 媒体类型。三步猜，猜不出就当 file。
fn media_kind(kind: &Option<String>, mime: &Option<String>, url: &str) -> MediaKind {
    let classify = |s: &str| {
        let s = s.to_lowercase();
        if s.contains("image") || s.contains("photo") || s.contains("picture") {
            Some(MediaKind::Image)
        } else if s.contains("audio") || s.contains("sound") || s.contains("voice") {
            Some(MediaKind::Audio)
        } else if s.contains("video") || s.contains("movie") {
            Some(MediaKind::Video)
        } else {
            None
        }
    };
    // kind 字段可能是 "image" 也可能是完整 mime "image/png"
    if let Some(k) = kind.as_deref().and_then(classify) {
        return k;
    }
    if let Some(m) = mime.as_deref().and_then(classify) {
        return m;
    }
    // URL 扩展名兜底
    let lower = url.split('?').next().unwrap_or(url).to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" => MediaKind::Image,
        "mp3" | "wav" | "ogg" | "m4a" | "flac" | "aac" => MediaKind::Audio,
        "mp4" | "webm" | "mkv" | "mov" | "avi" => MediaKind::Video,
        _ => MediaKind::File,
    }
}

/// 显示名兜底：URL 最后一段路径（去掉 query）。
fn url_filename(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let seg = path.rsplit('/').next().unwrap_or("");
    let name = percent_decode(seg);
    if name.is_empty() { "未命名文件".into() } else { name }
}

/// 最小百分号解码：只处理常见 %XX，失败字符原样保留。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(h) = bytes.get(i + 1..i + 3) {
                if let Ok(v) = u8::from_str_radix(&String::from_utf8_lossy(h), 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 宽松媒体项 → 结构化附件。没有可用 url 的整条丢弃。
fn to_media(m: RawMedia) -> Option<MediaItem> {
    let url = non_empty(&m.url)
        .or_else(|| non_empty(&m.src))
        .or_else(|| non_empty(&m.href))?;
    // 只认 http(s) —— file:// 之类的客户端绝不代拉
    if !url.to_lowercase().starts_with("http://") && !url.to_lowercase().starts_with("https://") {
        return None;
    }
    let mime = non_empty(&m.mime).or_else(|| non_empty(&m.content_type));
    let kind_str = non_empty(&m.kind)
        .or_else(|| non_empty(&m.r#type))
        .or_else(|| non_empty(&m.media_type));
    let name = non_empty(&m.name)
        .or_else(|| non_empty(&m.filename))
        .or_else(|| non_empty(&m.title))
        .or_else(|| Some(url_filename(&url)));
    Some(MediaItem {
        kind: media_kind(&kind_str, &mime, &url),
        url,
        name,
        mime,
        size: m.size.filter(|s| *s > 0.0).map(|s| s as u64),
        poster: non_empty(&m.poster)
            .or_else(|| non_empty(&m.thumb))
            .or_else(|| non_empty(&m.thumbnail)),
    })
}

impl RawNotice {
    /// 归一化成客户端内部模型。返回 None 表示「这不是一条通知」。
    pub fn normalize(self) -> Option<TaskNotice> {
        let card = self.card.unwrap_or_default();

        // 先判断这到底是不是通知：一个字段都没有的帧（心跳、订阅确认）
        // 必须原样丢掉，不能凑出一条空气泡。
        let looks_like_notice = self.id.is_some()
            || self.task.is_some()
            || self.title.is_some()
            || self.status.is_some()
            || self.result.is_some()
            || self.progress.is_some()
            || (self.text.is_some() && self.topic.is_some())
            || self.ask.is_some()
            || self.question.is_some()
            || card.action.is_some()
            || !card.fields.is_empty();
        if !looks_like_notice {
            return None;
        }

        let topic = non_empty(&self.topic).or_else(|| non_empty(&self.channel));

        // 服务端示例里 text 是「任务 - 结果」一行拼接。拆开比整行塞进 result 有用：
        // 气泡的扫视定位点必须是一个短任务名。
        let text = non_empty(&self.text).or_else(|| non_empty(&card.text));
        let (text_task, text_result) = match text.as_deref() {
            Some(t) => match t.split_once(" - ") {
                Some((a, b)) => (Some(a.trim().to_string()), Some(b.trim().to_string())),
                None => (None, Some(t.to_string())),
            },
            None => (None, None),
        };

        let task = non_empty(&self.task)
            .or_else(|| non_empty(&self.title))
            .or_else(|| field_of(&card, TASK_LABELS))
            .or_else(|| text_task)
            .or_else(|| non_empty(&card.title))
            .unwrap_or_else(|| "任务".into());

        let result = non_empty(&self.result)
            .or_else(|| field_of(&card, RESULT_LABELS))
            .or_else(|| text_result)
            .unwrap_or_default();

        let mut actions: Vec<NoticeAction> = self
            .actions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| to_action(a, "open"))
            .collect();

        // card.action 是单数形式，补进数组；已经给了 actions 就不重复加。
        if actions.is_empty() {
            if let Some(a) = card.action.and_then(|a| to_action(a, "open")) {
                actions.push(a);
            }
        }
        if actions.is_empty() {
            for a in card.actions.unwrap_or_default() {
                if let Some(a) = to_action(a, "open") {
                    actions.push(a);
                }
            }
        }

        // data.url / url：服务端的机器可读链接字段（BROADCAST v1 对齐）。
        // 没有别的动作时补一个「打开链接 / 打开文件」主按钮；已有动作说明
        // 服务端自己安排了按钮，不越俎代庖。http(s) 与本机绝对路径都收
        // （路径由 sys_open 侧把关：存在性 + 拒绝可执行扩展名）。
        let data_url = non_empty(&self.url).or_else(|| {
            self.data.as_ref().and_then(|d| non_empty(&d.url))
        });
        if let Some(u) = data_url {
            let l = u.to_lowercase();
            let is_http = l.starts_with("http://") || l.starts_with("https://");
            let is_path = looks_like_path(&u);
            if (is_http || is_path) && actions.is_empty() {
                actions.push(NoticeAction {
                    id: "view".into(),
                    label: if is_http { "打开链接".into() } else { "打开文件".into() },
                    style: ActionStyle::Primary,
                    payload: Some(u),
                });
            }
        }

        // 问询：`ask` 对象优先；没有就把平铺的 question + options 拼一个。
        // 少于两个选项的当普通通知处理 —— 只有一个「选项」的问询只是按钮。
        let ask = {
            let raw = match self.ask {
                Some(a) => Some(a),
                None => self.question.as_ref().map(|q| RawAsk {
                    question: Some(q.clone()),
                    options: self.options.clone(),
                    ..Default::default()
                }),
            };
            raw.and_then(|a| {
                let question = non_empty(&a.question)
                    .or_else(|| non_empty(&a.prompt))
                    .or_else(|| non_empty(&a.text))
                    .unwrap_or_default();
                let opts = a
                    .options
                    .or(a.choices)
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, o)| to_option(o, i))
                    .take(4)
                    .collect::<Vec<_>>();
                if opts.len() < 2 {
                    return None;
                }
                Some(NoticeAsk {
                    question: if question.is_empty() { "需要你确认一下".into() } else { question },
                    options: opts,
                    deadline: a.deadline.map(|t| if t > 0 && t < 1_000_000_000_000 { t * 1000 } else { t }),
                    context: non_empty(&a.context),
                })
            })
        };

        // 进度：兼容 0–100 的百分比写法。服务端传 62 是最常见的错，
        // 直接当 0–1 处理会变成满格，比夹到区间更糟。
        let progress = self.progress.map(|p| {
            let v = if p > 1.0 && p <= 100.0 { p / 100.0 } else { p };
            (v as f32).clamp(0.0, 1.0)
        });

        let status = self
            .status
            .as_deref()
            .and_then(parse_status)
            .or_else(|| topic.as_deref().and_then(status_from_topic))
            .or_else(|| progress.map(|_| TaskStatus::Running))
            // 兜底 success 而不是 running：running 的气泡不会自动收起，
            // 猜错会留一条永远不消失的假「执行中」。
            .unwrap_or(TaskStatus::Success);

        // 时间戳：10 位当秒处理。服务端给秒是高频错误，
        // 与其显示 1970 年，不如自己换算。
        let timestamp = self.timestamp.map(|t| if t > 0 && t < 1_000_000_000_000 { t * 1000 } else { t });

        let executor = self
            .executor
            .map(to_executor)
            .or_else(|| non_empty(&card.source).map(|n| Executor { name: n, node: None, kind: ExecutorKind::Agent }))
            .unwrap_or_else(|| Executor { name: "Agent Center".into(), node: None, kind: ExecutorKind::Agent });

        let id = non_empty(&self.id).unwrap_or_else(|| {
            // 指纹只用「任务名 + 执行方」，不带 topic：服务端在 running 和
            // completed 两帧里 topic 必然不同，带进去就永远接不上，
            // 一条任务会在气泡里裂成两条。
            fingerprint(&[&task, &executor.name])
        });

        // 媒体：三个字段名都收，没有 url 的条目丢弃，最多 12 条防刷屏。
        let media = self
            .media
            .or(self.attachments)
            .or(self.files)
            .map(|items| {
                items
                    .into_iter()
                    .filter_map(to_media)
                    .take(12)
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        Some(TaskNotice {
            id,
            task,
            executor,
            status,
            result,
            detail: non_empty(&self.detail),
            progress,
            actions,
            ask,
            sticky: self.sticky,
            timestamp: timestamp.unwrap_or_else(crate::models::EchoMessage::now_ms),
            buddy_pack: non_empty(&self.buddy_pack),
            channel: topic.unwrap_or_default(),
            terminal: non_empty(&self.terminal),
            offline_since: self
                .offline_since
                .map(|t| if t > 0 && t < 1_000_000_000_000 { t * 1000 } else { t }),
            media,
            agent: non_empty(&self.agent)
                .or_else(|| non_empty(&self.agent_name))
                .or_else(|| non_empty(&self.agent_alias)),
        })
    }
}

/// 从已经解析好的 JSON 值里取通知。`{"notice":{...}}` 信封会被自动剥掉。
pub fn from_value(v: serde_json::Value) -> Option<TaskNotice> {
    let inner = match v.get("notice") {
        Some(n) if n.is_object() => n.clone(),
        _ => v,
    };
    serde_json::from_value::<RawNotice>(inner).ok()?.normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_field_is_taken_from_any_alias() {
        // 主字段 agent
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"t1","task":"x","status":"success","agent":"Soundao"}"#)
                .unwrap();
        assert_eq!(from_value(v).unwrap().agent.as_deref(), Some("Soundao"));
        // 别名 agent_name / agent_alias
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"t2","task":"x","status":"success","agent_name":"配音师老王"}"#,
        )
        .unwrap();
        assert_eq!(from_value(v).unwrap().agent.as_deref(), Some("配音师老王"));
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"t3","task":"x","status":"success","agent_alias":"Research Agent"}"#,
        )
        .unwrap();
        assert_eq!(
            from_value(v).unwrap().agent.as_deref(),
            Some("Research Agent")
        );
        // 空串当没有：不让「""」占住小字位
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"t4","task":"x","status":"success","agent":"  "}"#)
                .unwrap();
        assert_eq!(from_value(v).unwrap().agent, None);
    }

    #[test]
    fn card_only_frame_is_usable() {
        // 服务端当前真实会推的形状：没有 id / status / progress / timestamp
        let v: serde_json::Value = serde_json::from_str(
            r#"{
              "topic": "task.completed",
              "text": "生成 Q3 市场趋势分析 - 报告已生成，共 26 页",
              "card": {
                "title": "任务完成",
                "source": "Research Agent",
                "fields": [{"label": "任务", "value": "生成 Q3 市场趋势分析"}],
                "action": {"label": "查看结果", "url": "https://example.com/q3"}
              }
            }"#,
        )
        .unwrap();
        let n = from_value(v).expect("应当归一化成功");
        assert_eq!(n.status, TaskStatus::Success, "topic 应映射成 success");
        assert_eq!(n.task, "生成 Q3 市场趋势分析");
        assert_eq!(n.executor.name, "Research Agent");
        assert_eq!(n.result, "报告已生成，共 26 页");
        assert_eq!(n.actions.len(), 1);
        assert_eq!(n.actions[0].id, "open");
        assert_eq!(n.actions[0].payload.as_deref(), Some("https://example.com/q3"));
        assert!(n.timestamp > 1_600_000_000_000, "缺时间戳应补当前毫秒");
    }

    #[test]
    fn percent_progress_is_divided() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"t1","task":"x","status":"running","progress":62}"#).unwrap();
        assert_eq!(from_value(v).unwrap().progress, Some(0.62));
    }

    #[test]
    fn seconds_timestamp_is_upgraded() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"t1","task":"x","timestamp":1790000000}"#).unwrap();
        assert_eq!(from_value(v).unwrap().timestamp, 1_790_000_000_000);
    }

    #[test]
    fn data_url_becomes_open_action() {
        // BROADCAST v1：text 给人看（夹裸 URL），data.url 给程序读。
        // 没有自带动作时补「打开链接」；只认 http(s)。
        let v: serde_json::Value = serde_json::from_str(
            r#"{
              "topic": "task.completed",
              "text": "首推完成 - 详见 https://gitea.example.com/bot/x",
              "data": {"url": "https://gitea.example.com/bot/x/commit/d55ab62"}
            }"#,
        )
        .unwrap();
        let n = from_value(v).expect("data.url 帧应是通知");
        assert_eq!(n.actions.len(), 1);
        assert_eq!(n.actions[0].id, "view");
        assert_eq!(n.actions[0].style, ActionStyle::Primary);
        assert_eq!(
            n.actions[0].payload.as_deref(),
            Some("https://gitea.example.com/bot/x/commit/d55ab62")
        );

        // 已有动作时不覆盖；非 http 的 url 不给按钮
        let with_actions = r#"{"id":"t","task":"x","actions":[{"id":"a","label":"接单"}],"data":{"url":"https://e.com"}}"#;
        let n2 = from_value(serde_json::from_str::<serde_json::Value>(with_actions).unwrap()).unwrap();
        assert_eq!(n2.actions[0].id, "a", "已有动作不该被 data.url 顶掉");

        let bad = r#"{"topic":"task.completed","text":"x","data":{"url":"file:///C:/x"}}"#;
        let n3 = from_value(serde_json::from_str::<serde_json::Value>(bad).unwrap()).unwrap();
        assert!(n3.actions.is_empty(), "file: 协议不给打开按钮");
    }

    #[test]
    fn path_data_url_becomes_open_file() {
        // 渲染节点把产物路径放进 data.url：本机路径也要能开（「打开文件」）。
        // file:// 伪协议不收 —— 前缀伪装成路径的一律拒绝。
        let v: serde_json::Value = serde_json::from_str(
            r#"{"topic":"task.completed","text":"渲染完成 - G:\\out\\report.mp4","data":{"url":"G:\\out\\report.mp4"}}"#,
        )
        .unwrap();
        let n = from_value(v).expect("路径型 data.url 应是通知");
        assert_eq!(n.actions.len(), 1);
        assert_eq!(n.actions[0].label, "打开文件");
        assert_eq!(n.actions[0].payload.as_deref(), Some("G:\\out\\report.mp4"));
    }

    #[test]
    fn control_frame_is_dropped() {
        // 心跳 / 订阅确认不能变成气泡
        for s in [r#"{"op":"pong"}"#, r#"{"op":"subscribed","topic":"task"}"#, r#"{}"#] {
            let v: serde_json::Value = serde_json::from_str(s).unwrap();
            assert!(from_value(v).is_none(), "不应把控制帧当通知: {s}");
        }
    }

    #[test]
    fn same_content_same_fingerprint() {
        let a = from_value(serde_json::from_str::<serde_json::Value>(
            r#"{"topic":"task.running","text":"x","card":{"source":"A"}}"#,
        ).unwrap()).unwrap();
        let b = from_value(serde_json::from_str::<serde_json::Value>(
            r#"{"topic":"task.running","text":"x","card":{"source":"A"}}"#,
        ).unwrap()).unwrap();
        assert_eq!(a.id, b.id, "重复推同样内容必须原地覆盖");
        assert!(a.id.starts_with("auto-"));
    }

    #[test]
    fn ask_needs_two_options() {
        // 一个选项只是按钮，不构成问询；否则气泡会永远停在「等你答复」
        let one = r#"{"id":"t1","task":"x","ask":{"question":"发不发？","options":[{"id":"go","label":"发"}]}}"#;
        let n = from_value(serde_json::from_str::<serde_json::Value>(one).unwrap()).unwrap();
        assert!(n.ask.is_none(), "单个选项不该被当成问询");

        let two = r#"{"id":"t2","task":"x","ask":{"question":"发不发？","options":[
            {"id":"go","label":"发","style":"primary"},{"id":"no","label":"不发"}]}}"#;
        let n = from_value(serde_json::from_str::<serde_json::Value>(two).unwrap()).unwrap();
        let ask = n.ask.as_ref().expect("两个选项应构成问询");
        assert_eq!(ask.question, "发不发？");
        assert_eq!(ask.options.len(), 2);
        assert_eq!(ask.options[0].id, "go");
        assert_eq!(ask.options[0].style, ActionStyle::Primary);
        assert!(n.is_sticky(), "问询必须常驻");
    }

    #[test]
    fn flat_question_is_promoted() {
        // 服务端很可能不用嵌套对象，而是平铺 question + options
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"t3","task":"x","question":"要发布吗？","options":[
                {"id":"yes","label":"发布"},{"id":"no","label":"再等等"}]}"#,
        )
        .unwrap();
        let n = from_value(v).unwrap();
        assert_eq!(n.ask.unwrap().question, "要发布吗？");
    }

    #[test]
    fn media_is_parsed_with_fallbacks() {
        // kind 不给、名字给的是 filename、url 带百分号编码 —— 都要接得住
        let v: serde_json::Value = serde_json::from_str(
            r#"{
              "id":"t9","task":"出图",
              "media":[
                {"url":"https://s.example.com/a%20b/poster.png","size":2048.0},
                {"kind":"audio","src":"https://s.example.com/v.mp3","filename":"配音.mp3"},
                {"type":"video","href":"https://s.example.com/d.mp4","thumb":"https://s.example.com/t.jpg"},
                {"name":"坏数据","url":"javascript:alert(1)"},
                {"name":"没有url"}
              ]
            }"#,
        )
        .unwrap();
        let n = from_value(v).unwrap();
        let m = n.media.expect("媒体应被解析");
        assert_eq!(m.len(), 3, "非 http url 和缺 url 的条目应丢弃");
        assert_eq!(m[0].kind, MediaKind::Image, "无 kind 时按扩展名猜");
        assert_eq!(m[0].name.as_deref(), Some("poster.png"), "缺名时取 URL 最后一段并解码");
        assert_eq!(m[0].size, Some(2048));
        assert_eq!(m[1].kind, MediaKind::Audio);
        assert_eq!(m[1].name.as_deref(), Some("配音.mp3"));
        assert_eq!(m[2].kind, MediaKind::Video);
        assert_eq!(m[2].poster.as_deref(), Some("https://s.example.com/t.jpg"));
    }

    #[test]
    fn media_only_frame_is_still_a_notice() {
        // 只带 media 的帧也算通知（媒体本身就有信息量）
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"t10","task":"看图","media":[{"url":"https://s.example.com/x.png"}]}"#,
        )
        .unwrap();
        let n = from_value(v).unwrap();
        assert_eq!(n.media.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn deliver_task_maps_title_to_task() {
        // task.deliver 用的是 title，通知里用的是 task，同一个东西。
        // 不映射的话气泡上会显示「任务」两个字，用户完全不知道要干什么。
        let v: serde_json::Value = serde_json::from_str(
            r#"{
              "id": "task_001",
              "title": "只给渲染节点",
              "executor": {"name": "Agent Center", "kind": "agent"},
              "actions": [{"id": "accept", "label": "开始", "style": "primary"}]
            }"#,
        )
        .unwrap();
        let n = from_value(v).expect("投递任务应能归一化");
        assert_eq!(n.task, "只给渲染节点");
        assert_eq!(n.actions.len(), 1);
        assert_eq!(n.actions[0].label, "开始");
    }
}
