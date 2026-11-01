//! 内置的本地演示服务端。
//!
//! 第一次装上客户端的人手里往往还没有服务端：设置页一片空白，
//! 悬浮层永远安静，用户根本不知道这个程序装完是干嘛的。
//! 所以客户端自带一个单机演示服务端 —— 点「使用本地服务端」就把它拉起来，
//! 自动连上，立刻推一轮完整的任务通知让人看到效果。
//!
//! 定位是**演示**，不是本地服务端产品：演示序列每条连接只播一轮，
//! 播完保持连接静默；不落盘、不鉴权（只绑 127.0.0.1）、帧格式与真实
//! 服务端完全一致（走 wire.rs 同一条宽松解析路径），所以演示里看到的
//! 就是正式联调时看到的。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// 演示服务端状态，暴露给设置页显示。
#[derive(Debug, Clone, Serialize)]
pub struct LocalDemoStatus {
    pub running: bool,
    pub port: u16,
    pub clients: usize,
}

/// 进程内的运行句柄。`None` = 还没启动过。
/// 已启动就不重复 bind：按钮点多少次都只回报现有状态。
pub struct LocalDemoState(pub Mutex<Option<Running>>);

pub struct Running {
    pub port: u16,
    /// 当前活着的 WS 连接数。Arc 是为了克隆进每条连接的任务里加减。
    pub clients: std::sync::Arc<AtomicUsize>,
}

fn status_of(running: Option<&Running>) -> Option<LocalDemoStatus> {
    running.map(|r| LocalDemoStatus {
        running: true,
        port: r.port,
        clients: r.clients.load(Ordering::Relaxed),
    })
}

/// 启动本地演示服务端并回报状态。重复调用幂等。
#[tauri::command]
pub async fn localdemo_start(app: AppHandle) -> Result<LocalDemoStatus, String> {
    if let Some(state) = app.try_state::<LocalDemoState>() {
        // guard 绑在变量上而不是塞进闭包，借用才能活到 return
        if let Ok(g) = state.0.lock() {
            if let Some(s) = status_of(g.as_ref()) {
                return Ok(s);
            }
        }
    }

    // 从 8790 起往后找 6 个口：8790 被别的程序占了不该直接失败。
    let mut bound = None;
    for p in 8790..=8795 {
        match TcpListener::bind(("127.0.0.1", p)).await {
            Ok(l) => {
                bound = Some((l, p));
                break;
            }
            Err(_) => continue,
        }
    }
    let Some((listener, port)) = bound else {
        return Err("127.0.0.1 的 8790–8795 端口都被占用，起不了本地演示服务端".into());
    };

    let clients = std::sync::Arc::new(AtomicUsize::new(0));
    if let Some(state) = app.try_state::<LocalDemoState>() {
        if let Ok(mut g) = state.0.lock() {
            *g = Some(Running {
                port,
                clients: clients.clone(),
            });
        }
    }

    // 接受循环与进程同寿命：演示服务端没有「停」的需求，
    // 客户端退出时任务跟着运行时一起没了。
    tokio::spawn(async move {
        loop {
            let Ok((stream, _addr)) = listener.accept().await else {
                break;
            };
            let clients = clients.clone();
            tokio::spawn(async move {
                clients.fetch_add(1, Ordering::Relaxed);
                if let Err(e) = serve_client(stream).await {
                    eprintln!("[localdemo] 演示连接异常结束: {e}");
                }
                clients.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });

    eprintln!("[localdemo] 演示服务端已启动 ws://127.0.0.1:{port}/ws");
    Ok(LocalDemoStatus { running: true, port, clients: 0 })
}

/// 当前演示服务端状态。没启动过就是 running:false。
#[tauri::command]
pub fn localdemo_status(app: AppHandle) -> LocalDemoStatus {
    // 不能写成 and_then 闭包链：guard 借用的是闭包里的临时值，活不过闭包。
    let Some(state) = app.try_state::<LocalDemoState>() else {
        return LocalDemoStatus { running: false, port: 0, clients: 0 };
    };
    let Ok(guard) = state.0.lock() else {
        return LocalDemoStatus { running: false, port: 0, clients: 0 };
    };
    status_of(guard.as_ref()).unwrap_or(LocalDemoStatus { running: false, port: 0, clients: 0 })
}

/// 一条演示连接：完成握手 → 播一轮任务生命周期 → 静默保活。
async fn serve_client(stream: tokio::net::TcpStream) -> Result<(), String> {
    let mut ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| format!("握手失败: {e}"))?;

    // 一轮完整生命周期：接单 → 推进 → 完成。字段形状与真实服务端一致，
    // 前端不需要为演示走任何特殊分支。
    let demo = |status: &str, progress: Option<f64>, result: &str| {
        let mut v = serde_json::json!({
            "id": "demo-task-1",
            "task": "体验一条任务通知",
            "status": status,
            "result": result,
            "executor": { "name": "本地演示", "kind": "local" },
            "timestamp": crate::models::EchoMessage::now_ms(),
        });
        if let Some(p) = progress {
            v["progress"] = serde_json::json!(p);
        }
        v.to_string()
    };

    let seq: [String; 4] = [
        demo("running", Some(0.12), "收到任务，开始执行（这是内置演示服务端推的）"),
        demo("running", Some(0.55), "正在生成预览…"),
        demo("running", Some(0.85), "即将完成…"),
        demo("success", None, "演示完成。真实任务会由你配置的服务端推过来"),
    ];
    for (i, frame) in seq.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1400)).await;
        }
        ws.send(Message::Text(frame.clone().into()))
            .await
            .map_err(|e| e.to_string())?;
    }

    // 播完保持连接：客户端的 hello/subscribe 帧都收下但不响应，
    // 读循环同时也是保活（pong 由库自动排队）。
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(Message::Text(_)) | Ok(Message::Binary(_)) | Ok(Message::Pong(_)) => continue,
            Ok(Message::Ping(p)) => {
                let _ = ws.send(Message::Pong(p)).await;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
        }
    }
    Ok(())
}
