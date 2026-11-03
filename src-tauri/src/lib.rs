mod activate;
mod buddy;
mod conn;
mod device;
mod guides;
mod localdemo;
mod media;
mod mock;
mod models;
mod overlay;
mod proto;
mod settings;
mod sysopen;
mod traymenu;
mod wire;

use tauri::{
    tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, PhysicalPosition, WindowEvent,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // 设置先以默认值占位，setup 里立刻用磁盘上的真实配置覆盖它。
        .manage(settings::SettingsState(std::sync::Mutex::new(
            settings::AppSettings::default(),
        )))
        .manage(conn::ConnState::default())
        .manage(localdemo::LocalDemoState(std::sync::Mutex::new(None)))
        // 有了托盘之后，关主窗口 = 缩到托盘继续守着通知；
        // 真正退出走托盘菜单的「退出」。悬浮窗不会变孤儿——
        // 它本来就是通知本体，主窗口藏起来它还得干活。
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // 拦住关闭，藏进托盘。第一次遇到的人可能以为程序没关掉，
                    // 所以托盘悬停提示里写清楚。
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = settings::load(&handle);
            if let Some(state) = handle.try_state::<settings::SettingsState>() {
                if let Ok(mut g) = state.0.lock() {
                    *g = cfg.clone();
                }
            }

            // ── 托盘菜单窗口 ────────────────────────────────
            // 原生菜单是系统绘制的，样式与应用主题无关；菜单本体做成
            // 一个气泡同款的小窗口，提前建好，点击时摆位弹出。
            if let Err(e) = traymenu::ensure(&handle) {
                eprintln!("[tray] {e}（托盘菜单将不可用，其余功能不受影响）");
            }

            // 托盘图标 = robot 头像特写（icono 泛应用图标的替代品，用户一眼认出形象）。
            // PNG 编进二进制，运行期解码一次；坏图标属于「装包不完整」，直接崩给开发者看。
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray.png"
            ))
            .expect("托盘图标 tray.png 解码失败");

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .tooltip("EchoCast — 关闭窗口后仍在托盘运行")
                // 菜单不走系统绘制：左/右键都弹自绘菜单窗口
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    // 只认「抬起」：按下就弹会跟系统对双击的判定打架
                    let TrayIconEvent::Click {
                        rect,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    else {
                        return;
                    };
                    let app = tray.app_handle();
                    let Some(win) = app.get_webview_window("traymenu") else {
                        eprintln!("[tray] 菜单窗口不存在（创建失败？）");
                        return;
                    };
                    // rect 是托盘图标的矩形：菜单底部对齐图标顶部，
                    // 水平方向把菜单中心对到图标中心。越界钳回屏幕内。
                    // Position/Size 是 Physical|Logical 枚举，托盘侧实际给物理值。
                    use tauri::{Position, Size};
                    let (icon_x, icon_y) = match rect.position {
                        Position::Physical(p) => (p.x, p.y),
                        Position::Logical(l) => (l.x.round() as i32, l.y.round() as i32),
                    };
                    let icon_w = match rect.size {
                        Size::Physical(s) => s.width as i32,
                        Size::Logical(l) => l.width.round() as i32,
                    };
                    let (lw, lh) = traymenu::logical_size();
                    let scale = win.scale_factor().unwrap_or(1.0);
                    let pw = (lw * scale).round() as i32;
                    let ph = (lh * scale).round() as i32;
                    let mut x = icon_x + (icon_w - pw) / 2;
                    let mut y = icon_y - ph - 8;
                    if let Ok(Some(m)) = app.primary_monitor() {
                        let (mw, mh) = (m.size().width as i32, m.size().height as i32);
                        x = x.clamp(8, (mw - pw - 8).max(8));
                        y = y.clamp(8, (mh - ph - 8).max(8));
                    }
                    let _ = win.set_position(PhysicalPosition::new(x, y));
                    let _ = win.show();
                    let _ = win.set_focus();
                })
                .build(app)?;

            let has_server = !cfg.server_url.is_empty();

            // mock 循环一直在，但每轮自己判断该不该推（见 mock::should_run）——
            // 开关要能随时生效，启动时一次性决定做不到这一点。
            #[cfg(debug_assertions)]
            mock::spawn_feed(handle.clone());

            if cfg.auto_connect && has_server {
                conn::start_delayed(
                    handle.clone(),
                    cfg.server_url.clone(),
                    std::time::Duration::from_millis(1200),
                );
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            buddy::buddy_user_packs,
            buddy::buddy_user_dir,
            mock::mock_push,
            settings::settings_get,
            settings::settings_set,
            settings::settings_path,
            conn::conn_status,
            conn::conn_connect,
            conn::conn_disconnect,
            conn::conn_send_action,
            conn::conn_send_ack,
            conn::debug_frames,
            conn::debug_clear,
            conn::terminal_get,
            activate::terminal_activate,
            overlay::overlay_apply,
            localdemo::localdemo_start,
            localdemo::localdemo_status,
            device::device_info,
            guides::guides_dir,
            guides::forge_dir,
            guides::open_local_dir,
            media::media_fetch,
            media::media_open,
            sysopen::sys_open,
            traymenu::tray_toggle_sound,
            traymenu::tray_menu_hide,
            traymenu::show_main_settings,
            traymenu::show_main_history,
            traymenu::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
