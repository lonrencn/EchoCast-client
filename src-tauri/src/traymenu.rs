//! 托盘菜单窗口。
//!
//! 原生托盘菜单是系统绘制的，样式跟应用主题完全无关 —— 用户要求
//! 菜单也要有气泡同款的玻璃质感，所以菜单本身做成一个独立小窗口：
//! 透明、无边框、置顶、不进任务栏，点托盘图标时弹到图标上方。
//!
//! 失焦自动收起（前端 blur 事件回调 tray_menu_hide），行为和原生菜单一致。

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// 与主窗口一致；autoplay 那条让提示音试听在这个窗口也能响。
const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// 菜单窗口逻辑尺寸。高了留白难看，按菜单项数量收口。
const MENU_W: f64 = 264.0;
const MENU_H: f64 = 262.0;

/// 建托盘菜单窗口。幂等：已存在直接返回。
/// 在 setup 里调用 —— Tauri 官方支持在 setup 建窗（与同步命令里建窗死锁是两回事）。
pub fn ensure(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window("traymenu") {
        return Ok(w);
    }
    let script = "window.__ECHOCAST_LABEL__='traymenu';";
    let builder = WebviewWindowBuilder::new(app, "traymenu", WebviewUrl::App("index.html".into()))
        .title("EchoCast")
        .inner_size(MENU_W, MENU_H)
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
        .initialization_script(script);
    builder
        .build()
        .map_err(|e| format!("创建托盘菜单窗口失败：{e}"))
}

/// 翻转提示音并广播。托盘菜单窗口调它，主窗口/悬浮层靠
/// `ec:settings-updated` 同步，三处显示永远一致。
#[tauri::command]
pub fn tray_toggle_sound(app: AppHandle) -> Result<bool, String> {
    let now_on = {
        let Some(state) = app.try_state::<crate::settings::SettingsState>() else {
            return Err("设置状态不可用".into());
        };
        let Ok(mut g) = state.0.lock() else {
            return Err("设置状态被占用".into());
        };
        g.sound_enabled = !g.sound_enabled;
        g.sound_enabled
    };
    let snap = crate::settings::current(&app);
    if let Err(e) = crate::settings::save(&app, &snap) {
        eprintln!("[traymenu] 静音写盘失败（本次运行仍生效）: {e}");
    }
    let _ = app.emit(
        "ec:settings-updated",
        serde_json::json!({ "sound_enabled": now_on }),
    );
    Ok(now_on)
}

/// 菜单收起。前端在 blur / Esc / 点完菜单项时调。
#[tauri::command]
pub fn tray_menu_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window("traymenu") {
        let _ = w.hide();
    }
}

/// 「设置…」：托盘菜单没有输入框，凡是配置类操作都跳主窗口。
#[tauri::command]
pub fn show_main_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    let _ = app.emit_to("main", "ec:open-settings", ());
    if let Some(w) = app.get_webview_window("traymenu") {
        let _ = w.hide();
    }
}

/// 「消息回看」：打开主窗口并切到回看列表。
/// 与设置同一套路：主窗口自己听事件切视图，托盘菜单只负责把人带过去。
#[tauri::command]
pub fn show_main_history(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    let _ = app.emit_to("main", "ec:show-history", ());
    if let Some(w) = app.get_webview_window("traymenu") {
        let _ = w.hide();
    }
}

/// 退出整个应用（含悬浮层、托盘菜单窗口）。
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// 菜单窗口的逻辑尺寸，lib.rs 摆位时用。
pub fn logical_size() -> (f64, f64) {
    (MENU_W, MENU_H)
}
