use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::models::BuddyPackInfo;

/// 缺任何一个动作，这个包就不合格 —— 否则会出现「失败配着笑脸」这种
/// 最致命的错配。宁可整包不显示，也不能带着缺陷渲染。
const REQUIRED_ACTIONS: [&str; 3] = ["idle", "success", "fail"];

/// 用户素材包根目录：$APPDATA/buddies
///
/// 内置包不在这里 —— 它们属于前端构建产物（src/buddies/builtin），
/// 打包后跟着 dist 走，不依赖运行时路径解析。
pub fn user_pack_root(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("buddies"))
}

/// 确保用户目录存在。首次运行时前端要能直接把文件丢进去。
pub fn ensure_user_dir(app: &AppHandle) -> Option<PathBuf> {
    let root = user_pack_root(app)?;
    std::fs::create_dir_all(&root).ok()?;
    Some(root)
}

/// 读取并校验一个包目录。读不出来就返回 None，不抛错 ——
/// 一个坏包不应该让整个列表加载失败。
fn read_pack(dir: &Path, source: &str) -> Option<BuddyPackInfo> {
    let manifest_path = dir.join("manifest.json");
    let raw = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest: Value = serde_json::from_str(&raw).ok()?;

    let id = manifest
        .get("pack")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            dir.file_name()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })?;

    let actions = manifest.get("actions").and_then(Value::as_object);
    let mut missing = Vec::new();
    for name in REQUIRED_ACTIONS {
        let present = actions
            .map(|a| a.get(name).and_then(|v| v.get("file")).is_some())
            .unwrap_or(false);
        if !present {
            missing.push(name.to_string());
        }
    }

    Some(BuddyPackInfo {
        id,
        source: source.to_string(),
        manifest,
        dir: Some(dir.to_string_lossy().to_string()),
        valid: missing.is_empty(),
        missing,
    })
}

/// 扫描用户目录下的所有包。
pub fn list_user_packs(app: &AppHandle) -> Vec<BuddyPackInfo> {
    let Some(root) = user_pack_root(app) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut out: Vec<BuddyPackInfo> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| read_pack(&p, "user"))
        .collect();

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 列出用户导入的形象包。前端会把它和内置包合并成一个列表。
#[tauri::command]
pub fn buddy_user_packs(app: AppHandle) -> Vec<BuddyPackInfo> {
    list_user_packs(&app)
}

/// 返回用户素材目录，并在需要时创建。前端用它做「打开文件夹」的落点。
#[tauri::command]
pub fn buddy_user_dir(app: AppHandle) -> Option<String> {
    ensure_user_dir(&app).map(|p| p.to_string_lossy().to_string())
}
