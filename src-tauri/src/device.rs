//! 本机设备身份。
//!
//! 路由的前提是「服务端知道这条连接是谁」。token 是凭证，但凭证要能对应到一台
//! 具体的机器，中间需要一个稳定的本地标识 —— 就是 `device_id`。
//!
//! 为什么是随机 UUID 而不是硬件指纹：
//! - 硬件指纹（主板序列号、MAC）在虚拟机上不稳定，改网卡就变一台「新机器」；
//! - 取 MAC 之类的信息在部分系统上需要额外权限，为一个标识不值得；
//! - 真需要「防重装」时，服务端可以拿 IP + 首次接入时间做人工核对。
//!
//! 存储位置跟 settings 同级（`$APPDATA/device.json`），所以**重装应用不换身份，
//! 重装系统才换** —— 这个语义正好，用户不需要理解任何东西。
//!
//! 没有 device_id 也能连（老用户升级上来就有），只是服务端会把它当成一台匿名终端。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// 本机稳定标识，首次启动生成，之后不再变化。
    pub device_id: String,
    /// 计算机名，服务端建终端时可以直接拿它当默认别名。
    pub hostname: String,
    /// `windows` / `macos` / `linux`
    pub os: String,
    pub app_version: String,
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("device.json"))
}

fn hostname() -> String {
    // Windows 上是 COMPUTERNAME，类 Unix 上是 HOSTNAME。
    // 都取不到就退回一个中性名字，别让整个 hello 帧因为一个展示字段失败。
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// 读本机身份；没有就生成一份写盘。
pub fn load_or_create(app: &AppHandle) -> DeviceInfo {
    if let Some(p) = path(app) {
        if let Ok(raw) = std::fs::read_to_string(&p) {
            if let Ok(mut d) = serde_json::from_str::<DeviceInfo>(&raw) {
                // hostname 可能被用户改过（改名、换域），每次以当前值为准：
                // device_id 才是身份，hostname 只是展示。
                d.hostname = hostname();
                d.app_version = env!("CARGO_PKG_VERSION").to_string();
                return d;
            }
        }
    }

    let fresh = DeviceInfo {
        device_id: uuid::Uuid::new_v4().to_string(),
        hostname: hostname(),
        os: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    };

    if let Some(p) = path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&fresh) {
            // 写失败不影响本次运行：device_id 变了最多让服务端多一条待清理的终端，
            // 而为此让应用起不来是完全不成比例的。
            if let Err(e) = std::fs::write(&p, raw) {
                eprintln!("[device] 身份文件写入失败，本次运行用临时身份: {e}");
            }
        }
    }

    fresh
}

/// 本机身份的只读快照。设置面板要显示它，所以开一个命令出去。
#[tauri::command]
pub fn device_info(app: AppHandle) -> DeviceInfo {
    load_or_create(&app)
}
