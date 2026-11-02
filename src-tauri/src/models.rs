// 这些结构目前只有定义、还没有调用方：连接管理与通知推送会在下一步接线。
// 用模块级 allow 而不是逐个标注，是为了让真正的死代码仍然能被看见。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// 连接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// 推送给前端的状态事件（事件名: ec:status）
#[derive(Debug, Clone, Serialize)]
pub struct StatusEvent {
    pub status: ConnStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// EchoCast 消息模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EchoMessage {
    /// 服务端消息 ID（本地生成时为 UUID）
    pub id: String,
    /// 频道 / 主题
    pub channel: String,
    /// 发送者节点名
    pub sender: String,
    /// 消息文本
    pub content: String,
    /// 毫秒级 Unix 时间戳
    pub timestamp: i64,
}

impl EchoMessage {
    pub fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    pub fn new_local(sender: &str, channel: &str, content: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel.to_string(),
            sender: sender.to_string(),
            content: content.to_string(),
            timestamp: Self::now_ms(),
        }
    }
}

// ---------------------------------------------------------------------------
// 任务结果通知
//
// 聊天消息（EchoMessage）装不下「任务 / 执行方 / 结果」这个语义，
// 所以这里是另一套模型。channel 只保留成分组字段。
// ---------------------------------------------------------------------------

/// 任务结果状态。与前端气泡的状态色一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Success,
    Warning,
    Failed,
}

impl TaskStatus {
    /// 失败与警告必须常驻：用户没看见就等于没通知。
    pub fn is_sticky(self) -> bool {
        matches!(self, TaskStatus::Failed | TaskStatus::Warning)
    }

    /// 该状态是否需要展示进度条。
    pub fn has_progress(self) -> bool {
        matches!(self, TaskStatus::Running)
    }
}

/// 执行方类型，决定前端渲染哪种小图标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorKind {
    #[default]
    Local,
    Remote,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Executor {
    pub name: String,
    /// 节点名，例如「GPU 节点 02」。单机场景可以不给。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default)]
    pub kind: ExecutorKind,
}

impl Executor {
    /// 渲染成「名字 · 节点」，没有节点就只显示名字。
    pub fn display(&self) -> String {
        match &self.node {
            Some(n) if !n.is_empty() => format!("{} · {}", self.name, n),
            _ => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActionStyle {
    #[default]
    Ghost,
    Primary,
}

/// 气泡上的动作按钮。文案由服务端给，前端不硬编码「重试」这类词。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoticeAction {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub style: ActionStyle,
    /// 点击后回传给后端的载荷，可以是命令名或 URL。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

/// 问询的一个选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub style: ActionStyle,
    /// 选中后回传给服务端的载荷。为空则回传 option id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

/// 问询：这条任务需要人给一个答复。
///
/// 和 `actions` 的区别是语义：`actions` 是「你可以做点什么」，点完就结束；
/// `ask` 是「我在等你选一个」，答复之前这条任务不算走完 —— 所以前端会
/// 常驻、会用另一套提示音（重复三连），并且在气泡上明说「等你答复…」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoticeAsk {
    /// 一句话问题。气泡放不下，只在展开面板里出现。
    #[serde(default)]
    pub question: String,
    /// 2–4 个选项。少于 2 个不成立（只有一个选择就不叫问询）。
    #[serde(default)]
    pub options: Vec<AskOption>,
    /// 答复截止时间（毫秒）。过期只是提示，仍然可答。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<i64>,
    /// 原样回传给服务端的上下文，客户端不解释。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl NoticeAsk {
    pub fn is_usable(&self) -> bool {
        self.options.len() >= 2
    }
}

/// 媒体附件类型。服务端没给时客户端按 mime / URL 扩展名猜。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    #[default]
    File,
}

/// 通知携带的多媒体附件。
///
/// url 必须是可 HTTP GET 拉取的资源（客户端不支持 base64 内嵌 ——
/// 内嵌会让 WS 文本帧膨胀几十倍，缓冲和解析两头受罪）。
/// 下载由 Rust 侧完成并缓存，前端只拿本地路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    #[serde(default)]
    pub kind: MediaKind,
    pub url: String,
    /// 显示名。缺省时客户端用 URL 最后一段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 帮助判断类型；kind 缺省时才有用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// 字节数，只用于展示（「3.2 MB」）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// 视频封面图 URL，可空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
}

/// 一条任务结果通知，推送给前端渲染成气泡。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNotice {
    pub id: String,
    /// 任务名。前端单行省略，是整个气泡唯一的扫视定位点。
    pub task: String,
    pub executor: Executor,
    pub status: TaskStatus,
    /// 结果摘要，两行封顶。只陈述事实，不复述「成功/失败」。
    #[serde(default)]
    pub result: String,
    /// 展开后的完整内容或日志尾部。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 0.0–1.0，仅 Running 有意义。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(default)]
    pub actions: Vec<NoticeAction>,
    /// 需要人答复的任务。有它时前端把动作区换成「等你答复…」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<NoticeAsk>,
    /// 覆盖默认的常驻判定。None 表示按 status 推断。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky: Option<bool>,
    /// 毫秒级 Unix 时间戳。
    pub timestamp: i64,
    /// 指定使用哪套形象包；None 表示跟随全局设置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buddy_pack: Option<String>,
    /// 分组，对应频道概念。
    #[serde(default)]
    pub channel: String,
    /// 这条任务被路由到哪台终端，形如 `term_7f3a…`。
    ///
    /// 客户端的路由逻辑是「服务端发什么就收什么」，这个字段只用于展示与排障：
    /// 用户能一眼看出「这条是发给我的」还是「服务端投错了」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// 任务创建时终端不在线，上线后补投 —— 这里是原始创建时间（毫秒）。
    /// 非空时气泡上会标「离线期间的任务」，否则用户会以为刚刚才发生。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_since: Option<i64>,
    /// 多媒体附件。有值时前端展示媒体区（图片/音频/视频/文件卡片）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Vec<MediaItem>>,
    /// agent 属名（署名）。服务端下发，展示在状态标题后的小字；
    /// 超过 20 个字符由前端掩码。与 executor 分开：executor 是
    /// 「谁在跑」，agent_name 是「以谁的名义交付」。
    /// 对外字段名约定为 `agent_name` 或 `note_alias`（两者等价），
    /// 不用 `agent` —— 名字里带量词，服务端开发者一眼能懂。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
}

impl TaskNotice {
    pub fn is_sticky(&self) -> bool {
        // 问询必须常驻：没人答复它就永远不算结束，自己消失等于把这个球丢了
        if self.ask.as_ref().is_some_and(NoticeAsk::is_usable) {
            return true;
        }
        self.sticky.unwrap_or_else(|| self.status.is_sticky())
    }

    /// 进度更新：同一条通知原地更新，不新建气泡，避免刷屏。
    /// 状态一旦进入终态就不再被进度覆盖。
    pub fn apply_progress(&mut self, progress: f32, status: TaskStatus) {
        if matches!(self.status, TaskStatus::Success | TaskStatus::Failed) {
            return;
        }
        self.progress = Some(progress.clamp(0.0, 1.0));
        self.status = status;
    }

    /// 动作按钮最多两个，多出来的截断 —— 这是前端的布局硬约束，后端先保证。
    pub fn trim_actions(&mut self) {
        self.actions.truncate(2);
    }
}

/// 服务端签发的终端身份。客户端只读，不参与路由决策。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub id: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub labels: Vec<String>,
}

impl TerminalInfo {
    /// 界面上一行显示：优先别名，没有就用 id 尾部，绝不显示空字符串。
    pub fn display(&self) -> String {
        if !self.alias.trim().is_empty() {
            return self.alias.clone();
        }
        let tail: String = self.id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
        format!("终端 {tail}")
    }
}

/// 形象包元信息，供设置面板列出可选项。
#[derive(Debug, Clone, Serialize)]
pub struct BuddyPackInfo {
    pub id: String,
    /// builtin | user
    pub source: String,
    /// 原始 manifest，前端自己决定怎么读。
    pub manifest: serde_json::Value,
    /// 用户包的绝对目录；内置包为 None（走前端打包路径）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// 校验是否通过。不通过的不应出现在可选列表里。
    pub valid: bool,
    /// 缺失的动作名。
    pub missing: Vec<String>,
}

// ---------------------------------------------------------------------------
// 线路契约测试
//
// 这里不测业务逻辑，主要把真实序列化结果打印出来 —— 给服务端对接时，
// 手写的「大概长这样」和 serde 的实际输出经常差在 Option 字段被跳过这类细节上。
// 改字段结构时这个测试会跟着变，正好用来发现契约漂移。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod contract {
    use super::*;

    fn running() -> TaskNotice {
        TaskNotice {
            id: "task-8f3a-004".into(),
            task: "合成《传承的力量》第 4 段".into(),
            executor: Executor {
                name: "Soundao".into(),
                node: Some("Edge TTS".into()),
                kind: ExecutorKind::Local,
            },
            status: TaskStatus::Running,
            result: "正在合成第 3–4 段".into(),
            detail: None,
            progress: Some(0.62),
            actions: vec![],
            ask: None,
            sticky: None,
            timestamp: 1_790_000_000_000,
            buddy_pack: None,
            channel: "soundao".into(),
            terminal: Some("term_7f3a1c".into()),
            offline_since: None,
            media: None,
            agent_name: None,
        }
    }

    fn success() -> TaskNotice {
        let mut n = running();
        n.id = "task-8f3a-001".into();
        n.task = "生成 Q3 市场趋势分析".into();
        n.executor = Executor {
            name: "Research Agent".into(),
            node: None,
            kind: ExecutorKind::Agent,
        };
        n.status = TaskStatus::Success;
        n.result = "报告已生成，共 26 页".into();
        n.detail = Some("https://echocast.local/reports/q3-2026".into());
        n.progress = None;
        n.actions = vec![
            NoticeAction {
                id: "open".into(),
                label: "查看结果".into(),
                style: ActionStyle::Primary,
                payload: Some("https://echocast.local/reports/q3-2026".into()),
            },
            NoticeAction {
                id: "copy".into(),
                label: "复制路径".into(),
                style: ActionStyle::Ghost,
                payload: None,
            },
        ];
        n.buddy_pack = Some("robot".into());
        n
    }

    #[test]
    fn print_wire_contract() {
        println!(
            "=== running ===\n{}",
            serde_json::to_string_pretty(&running()).unwrap()
        );
        println!(
            "=== success ===\n{}",
            serde_json::to_string_pretty(&success()).unwrap()
        );
        println!(
            "=== failed ===\n{}",
            serde_json::to_string_pretty(&{
                let mut n = success();
                n.status = TaskStatus::Failed;
                n.result = "检测到非常规活动，已阻断".into();
                n
            })
            .unwrap()
        );
    }

    #[test]
    fn transitions_are_one_way() {
        // 终态不能被进度更新拉回 running，否则状态灯会来回跳
        let mut n = success();
        n.apply_progress(0.3, TaskStatus::Running);
        assert_eq!(n.status, TaskStatus::Success);
        assert!(n.progress.is_none(), "终态不该被写入进度");

        // running 可以原地推进，且夹在 0–1
        let mut r = running();
        r.apply_progress(1.4, TaskStatus::Running);
        assert_eq!(r.progress, Some(1.0));
    }

    #[test]
    fn actions_capped_at_two() {
        let mut n = success();
        n.actions = (0..4)
            .map(|i| NoticeAction {
                id: format!("a{i}"),
                label: format!("动作 {i}"),
                style: ActionStyle::Ghost,
                payload: None,
            })
            .collect();
        n.trim_actions();
        assert_eq!(n.actions.len(), 2);
    }

    #[test]
    fn sticky_defaults_by_status() {
        // 失败和警告必须憋着不走，用户没看见等于没通知
        let mut n = success();
        n.status = TaskStatus::Failed;
        assert!(n.is_sticky());
        n.status = TaskStatus::Warning;
        assert!(n.is_sticky());
        n.status = TaskStatus::Success;
        assert!(!n.is_sticky());
        // 显式覆盖优先
        n.sticky = Some(true);
        assert!(n.is_sticky());
    }
}

