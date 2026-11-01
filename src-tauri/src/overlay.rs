//! 桌面悬浮层：右下角气泡 + 顶部灯条。
//!
//! 通知不能渲染在主窗口里 —— 主窗口是个控制台，最小化或被盖住之后
//! 通知就跟着没了。所以真正的通知面是两个独立的窗口：
//!
//! - `overlay`：右下角，440×640，透明、无边框、置顶、不进任务栏；
//! - `topbar`：贴屏幕顶边的通条，宽 = 屏幕宽，高 = 54。
//!
//! 两个窗口常驻（隐藏/显示由 display_mode 决定），而不是按需创建 ——
//! Windows 上反复创建销毁带透明 WebView 的窗口会闪黑帧。

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

/// 主窗口配置里的同款参数。autoplay 那条尤其不能少：
/// 悬浮层窗口没被点过，WebView2 默认策略会拒播提示音。
const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// 气泡浮层的逻辑尺寸，和 NoticeStack 的设计稿一致。
const OVERLAY_W: f64 = 440.0;
const OVERLAY_H: f64 = 640.0;
/// 右边距、底边距。底部 72 是给任务栏留的：气泡压在任务栏上会被误点。
const MARGIN_R: f64 = 20.0;
const MARGIN_B: f64 = 72.0;
/// 灯条高度。和 TopBarStrip 的条高一致，窗口不留多余透明边。
const TOPBAR_H: f64 = 54.0;

fn ensure(app: &AppHandle, label: &str) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(label) {
        return Ok(w);
    }
    // 前端靠这个注入量区分自己是哪个窗口：主窗口走 tauri.conf.json，
    // 只有悬浮窗口需要它。脚本必须赶在页面任何脚本之前执行。
    let script = format!("window.__ECHOCAST_LABEL__='{label}';");
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("EchoCast")
        .inner_size(OVERLAY_W, OVERLAY_H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false)
        .visible(false)
        .additional_browser_args(BROWSER_ARGS)
        .initialization_script(&script);
    match builder.build() {
        Ok(w) => {
            eprintln!("[overlay] window {label} created");
            Ok(w)
        }
        Err(e) => {
            eprintln!("[overlay] window {label} build FAILED: {e}");
            Err(format!("创建悬浮窗口 {label} 失败：{e}"))
        }
    }
}

/// 按当前 display_mode 调整两个悬浮窗口的几何与可见性。
///
/// 幂等：窗口已存在就只改位置，不重建。主窗口在启动和切换形态时各调一次。
///
/// **必须是 async**：同步命令跑在主线程上，而建窗要等事件循环腾出手 ——
/// 同步命令里调 `build()` 会互相等死（首轮联调时进程就吊死在这）。
/// async 命令跑在线程池上，建窗请求正常派发回主线程。
#[tauri::command]
pub async fn overlay_apply(app: AppHandle, mode: String) -> Result<(), String> {
    let topbar_mode = mode == "topbar";
    eprintln!("[overlay] apply mode={mode}");

    let overlay = ensure(&app, "overlay")?;
    eprintln!("[overlay] overlay ready");
    let topbar = ensure(&app, "topbar")?;
    eprintln!("[overlay] topbar ready");

    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("找不到主显示器")?;
    let scale = monitor.scale_factor();
    // 物理像素换算成逻辑像素：悬浮层的尺寸都用逻辑值设，才不受 DPI 影响
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;
    eprintln!("[overlay] screen {screen_w}x{screen_h} scale={scale}");

    let _ = overlay.set_size(LogicalSize::new(OVERLAY_W, OVERLAY_H));
    let _ = overlay.set_position(LogicalPosition::new(
        (screen_w - OVERLAY_W - MARGIN_R).max(0.0),
        (screen_h - OVERLAY_H - MARGIN_B).max(0.0),
    ));

    let _ = topbar.set_size(LogicalSize::new(screen_w, TOPBAR_H));
    let _ = topbar.set_position(LogicalPosition::new(0.0, 0.0));

    if topbar_mode {
        let _ = topbar.show();
        let _ = overlay.hide();
    } else {
        let _ = overlay.show();
        let _ = topbar.hide();
    }
    eprintln!("[overlay] done, topbar_mode={topbar_mode}");
    Ok(())
}
