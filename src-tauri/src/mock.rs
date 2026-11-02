//! Mock 推送源：在没有真实服务端的情况下，模拟节点把任务通知推下来。
//!
//! 这里刻意不绕开 `TaskNotice` —— 前端收到的结构与真实链路上完全一致，
//! 以后接上 WebSocket，只需要替换 `emit` 的调用点，前后端都不用改。

use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::models::{
    ActionStyle, AskOption, Executor, ExecutorKind, NoticeAction, NoticeAsk, TaskNotice, TaskStatus,
};

/// 与前端约定的通知事件名。
pub const NOTICE_EVENT: &str = "ec:notice";

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 一个 mock 场景：任务名、执行方、终态、结果，以及执行中的逐步说明。
struct Scenario {
    task: &'static str,
    executor: Executor,
    status: TaskStatus,
    /// 执行中的说明文案，按进度取，制造「它真的在动」的感觉。
    ticking: &'static [&'static str],
    result: &'static str,
    actions: &'static [(&'static str, &'static str, ActionStyle)],
    /// 需要人答复时的问题与选项。有它就说明这条任务卡在人这里。
    ask: Option<(&'static str, &'static [(&'static str, &'static str, ActionStyle)])>,
    channel: &'static str,
}

fn who(name: &str, node: Option<&str>, kind: ExecutorKind) -> Executor {
    Executor {
        name: name.to_string(),
        node: node.map(str::to_string),
        kind,
    }
}

/// 四个场景轮换，覆盖四种终态。内容取自真实业务，避免演示时全是 Lorem。
fn scenario(round: usize) -> Scenario {
    match round % 4 {
        0 => Scenario {
            task: "生成 Q3 市场趋势分析",
            executor: who("Research Agent", None, ExecutorKind::Agent),
            status: TaskStatus::Success,
            ticking: &["抓取 42 个数据源", "清洗与对齐口径", "生成图表 18 张", "校对引用"],
            result: "报告已生成，共 26 页",
            actions: &[
                ("open", "查看结果", ActionStyle::Primary),
                ("copy", "复制路径", ActionStyle::Ghost),
            ],
            ask: None,
            channel: "research",
        },
        1 => Scenario {
            task: "方言保留微调 · 第 3 轮",
            executor: who("GPU 节点 02", Some("ACE-Step"), ExecutorKind::Remote),
            status: TaskStatus::Warning,
            ticking: &["加载语料 12.4 GB", "第 3 轮反向传播", "校验音质", "导出权重"],
            result: "显存峰值 23.8 / 24 GB，batch 降到 4 可重试",
            // 这一条是问询：显存贴顶属于「要不要继续」的判断，不是自动重试能决定的
            actions: &[],
            ask: Some((
                "显存峰值 23.8 / 24 GB，已经贴顶。降 batch 重跑，还是今晚先停？",
                &[
                    ("retry-low", "降到 batch 4 重跑", ActionStyle::Primary),
                    ("hold", "今晚先停", ActionStyle::Ghost),
                ],
            )),
            channel: "train",
        },
        2 => Scenario {
            task: "合成《传承的力量》第 4 段",
            executor: who("Soundao", Some("Edge TTS"), ExecutorKind::Local),
            status: TaskStatus::Success,
            ticking: &["切分 6 个语段", "合成第 1–2 段", "合成第 3–4 段", "混流与响度归一"],
            result: "1 分 42 秒，响度 -16 LUFS",
            actions: &[("open", "试听", ActionStyle::Primary)],
            ask: None,
            channel: "soundao",
        },
        _ => Scenario {
            task: "检查异常登录",
            executor: who("Security Agent", None, ExecutorKind::Agent),
            status: TaskStatus::Failed,
            ticking: &["拉取近 24h 鉴权日志", "比对设备指纹", "关联地理跳变", "生成处置建议"],
            result: "检测到非常规活动，已阻断",
            actions: &[
                ("open", "查看详情", ActionStyle::Primary),
                ("ignore", "忽略", ActionStyle::Ghost),
            ],
            ask: None,
            channel: "security",
        },
    }
}

impl Scenario {
    /// 终态通知。
    fn notice(&self, id: &str) -> TaskNotice {
        let mut n = TaskNotice {
            id: id.to_string(),
            task: self.task.to_string(),
            executor: self.executor.clone(),
            status: self.status,
            result: self.result.to_string(),
            detail: None,
            progress: None,
            actions: self
                .actions
                .iter()
                .map(|(id, label, style)| NoticeAction {
                    id: id.to_string(),
                    label: label.to_string(),
                    style: *style,
                    payload: None,
                })
                .collect(),
            ask: self.ask.map(|(question, options)| NoticeAsk {
                question: question.to_string(),
                options: options
                    .iter()
                    .map(|(id, label, style)| AskOption {
                        id: id.to_string(),
                        label: label.to_string(),
                        style: *style,
                        payload: None,
                    })
                    .collect(),
                deadline: None,
                context: None,
            }),
            sticky: None,
            timestamp: now_ms(),
            buddy_pack: None,
            channel: self.channel.to_string(),
            // mock 是本机造的数据，没有真实终端路由，这两个字段留空
            terminal: None,
            offline_since: None,
            media: None,
            agent: None,
        };
        n.trim_actions();
        n
    }

    /// 执行中通知：同 id 原地更新，前端只换内容不新建气泡。
    /// 执行中不给动作 —— 这时候点「重试」只会重复触发一次任务。
    fn running(&self, id: &str, progress: f32) -> TaskNotice {
        let mut n = self.notice(id);
        n.status = TaskStatus::Running;
        n.progress = Some(progress);
        n.actions.clear();
        // 执行中还没到「等你答复」那一步，别提前催
        n.ask = None;
        n.result = self.ticking_text(progress).to_string();
        n
    }

    fn ticking_text(&self, progress: f32) -> &'static str {
        if self.ticking.is_empty() {
            return "";
        }
        let i = (progress * self.ticking.len() as f32) as usize;
        self.ticking[i.min(self.ticking.len() - 1)]
    }
}

fn emit(app: &AppHandle, n: &TaskNotice) {
    if let Err(e) = app.emit(NOTICE_EVENT, n) {
        eprintln!("[mock] 推送失败: {e}");
    }
}

/// mock 该不该说话。
///
/// 三条同时成立才推：用户开着开关、没配服务端地址、也确实没连着。
/// 「配了地址就不再 mock」是刻意的：真假两路同时推，用户分不清哪条是真的，
/// 排障时这种混淆比没有数据更糟。
fn should_run(app: &AppHandle) -> bool {
    let s = crate::settings::current(app);
    s.mock_enabled && s.server_url.is_empty() && !crate::conn::is_live(app)
}

/// 启动 mock 推送。每个任务走一遍「执行中 → 终态」，然后换下一个场景。
pub fn spawn_feed(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // 窗口要先建立前端订阅，否则第一条推送会落在订阅之前被丢掉
        tokio::time::sleep(Duration::from_millis(1500)).await;

        let mut round = 0usize;
        loop {
            if !should_run(&app) {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                continue;
            }

            let sc = scenario(round);
            // id 按场景固定，不按轮次递增 —— 同一任务再跑一次是「更新」，
            // 不是又冒出一条新的。否则警告和失败会无限堆积在浮层里。
            let id = format!("mock-{}", round % 4);

            for p in [0.12f32, 0.34, 0.58, 0.81] {
                emit(&app, &sc.running(&id, p));
                tokio::time::sleep(Duration::from_millis(720)).await;
            }

            emit(&app, &sc.notice(&id));
            round += 1;

            // 停留时间要够看完，又不能久到让人以为卡住
            tokio::time::sleep(Duration::from_millis(4600)).await;
        }
    });
}

/// 手动推一条，用于在桌面上点一下就看到气泡，不必等定时器。
#[tauri::command]
pub fn mock_push(app: AppHandle, round: Option<usize>) -> Result<(), String> {
    let r = round.unwrap_or_else(|| (now_ms() / 1000) as usize);
    let sc = scenario(r);
    let n = sc.notice(&format!("mock-manual-{r}"));
    app.emit(NOTICE_EVENT, &n)
        .map_err(|e| format!("推送失败: {e}"))
}
