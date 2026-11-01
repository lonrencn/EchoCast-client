//! 随安装包分发的资料入口。
//!
//! 打包配置（tauri.conf.json `bundle.resources`）把两份东西装进安装目录：
//! - `resources/docs/`：服务端数据接口文档（notice-protocol / terminal-routing / buddies）
//! - `resources/buddy-forge/`：形象定制 skill（指南 + 管线脚本）
//!
//! 装好后用户从设置面板一键打开对应文件夹。文档是只读资料，
//! 用户真正做形象包要写到 `$APPDATA/buddies/`（见 buddy.rs）。

use std::path::{Path, PathBuf};

use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

/// 去掉 Windows `canonicalize` 给的 `\\?\` 原样前缀。
///
/// 不剥的话下面 `starts_with(buddies_dir)` 永远比不中：`\\?\C:\...` 的
/// 组件序列和 `C:\...` 完全不同，用户点「打开形象包文件夹」必然被拒，
/// 还只会看到一句莫名其妙的「只允许打开…」。
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => PathBuf::from(format!(r"\\{rest}")),
        None => match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => p.to_path_buf(),
        },
    }
}

/// 资源子目录解析。dev 下 resource_dir 指向 src-tauri，装包后指向安装目录，
/// 两种情况 resources/ 都在其下。
fn resource_subdir(app: &tauri::AppHandle, name: &str) -> Option<String> {
    let dir = app.path().resource_dir().ok()?.join("resources").join(name);
    if dir.is_dir() {
        Some(dir.to_string_lossy().to_string())
    } else {
        None
    }
}

/// 服务端接口文档目录。找不到（理论不会）返回 null，前端提示去仓库看。
#[tauri::command]
pub fn guides_dir(app: tauri::AppHandle) -> Option<String> {
    resource_subdir(&app, "docs")
}

/// 形象定制指南（buddy-forge skill）目录，内含 SKILL.md 和 scripts/。
#[tauri::command]
pub fn forge_dir(app: tauri::AppHandle) -> Option<String> {
    resource_subdir(&app, "buddy-forge")
}

/// 用系统默认方式打开本地路径（文件夹用资源管理器）。
///
/// 这里只打开**应用自己控制的两类目录**：随包资源与用户形象包目录，
/// 不做成通用「打开任意路径」入口 —— 那等于把本机文件系统暴露给渲染层。
#[tauri::command]
pub fn open_local_dir(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let allowed = [
        resource_subdir(&app, "docs"),
        resource_subdir(&app, "buddy-forge"),
    ];
    let target = strip_verbatim(&Path::new(&path).canonicalize().map_err(|e| e.to_string())?);
    let buddies = app
        .path()
        .app_data_dir()
        .map(|d| d.join("buddies"))
        .unwrap_or_default();
    let ok = allowed
        .iter()
        .flatten()
        .any(|a| {
            Path::new(a)
                .canonicalize()
                .map(|c| strip_verbatim(&c) == target)
                .unwrap_or(false)
        })
        || target.starts_with(&buddies);
    if !ok {
        return Err("这里只能打开应用自带的资料目录或你的形象包目录".into());
    }
    app.opener()
        .open_path(target.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}
