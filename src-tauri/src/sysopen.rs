// 「打开」的兜底通道。
//
// 为什么有了 plugin-opener 还要这条命令：前端调 opener 插件要过 ACL
// （opener:default 只放行 http/https/mailto/tel），ACL 没配对时点击是
// **静默失败** —— 用户看到的就是「点了没反应」。这条命令由我们自己的
// Rust 代码直接调 opener，绕过前端 ACL，等于把「能不能开」的判断收进
// 代码里，配错 capability 也不会再让点击失效。
//
// 安全边界（取代 ACL 的那道墙，必须比 ACL 更明确）：
// - URL 只放行 http/https/mailto，其余协议（file:/javascript:/…）一律拒绝；
// - 本机路径放行**打开**，但拒绝一切可执行扩展名 —— 远程下发的路径
//   绝不能变成「在用户机器上执行程序」的入口；
// - 路径必须真实存在。渲染节点上的产物路径开不了就明说「不在这台机器」，
//   比 shell 静默失败强得多。

use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// 允许直接打开的 URL 协议。
const URL_PREFIXES: &[&str] = &["http://", "https://", "mailto:"];

/// 绝不代开的可执行扩展名。宁可错杀：远程输入打开一个 exe 的代价太大。
const BLOCKED_EXTS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "pif", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf",
    "wsh", "hta", "msi", "msp", "mst", "jar", "lnk", "reg", "gadget", "iso", "img",
];

fn scheme_allowed(s: &str) -> bool {
    let l = s.to_lowercase();
    URL_PREFIXES.iter().any(|p| l.starts_with(p))
}

fn ext_blocked(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_lowercase();
            BLOCKED_EXTS.iter().any(|b| *b == e)
        })
        .unwrap_or(false)
}

/// 用系统默认程序打开一个链接或本机路径。
///
/// 前端所有「打开」最终都汇到这里：链接进浏览器，路径进系统默认程序。
/// Err 的字符串会一路透传到气泡上的提示行，写给用户看，别返回调试黑话。
#[tauri::command]
pub async fn sys_open(app: AppHandle, target: String) -> Result<(), String> {
    let t = target.trim().to_string();
    if t.is_empty() {
        return Err("没有要打开的内容".into());
    }

    if scheme_allowed(&t) {
        return app
            .opener()
            .open_url(t, None::<&str>)
            .map_err(|e| format!("系统打不开这个链接: {e}"));
    }

    let path = std::path::PathBuf::from(&t);
    if !path.is_absolute() {
        return Err("只支持 http/https 链接或本机绝对路径".into());
    }
    if !path.exists() {
        return Err("这个文件不在这台机器上，打不开".into());
    }
    if ext_blocked(&path) {
        return Err("可执行文件不允许直接打开".into());
    }
    app.opener()
        .open_path(t, None::<&str>)
        .map_err(|e| format!("系统打不开这个文件: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_are_strict() {
        assert!(scheme_allowed("https://a.com/x"));
        assert!(scheme_allowed("HTTP://A.COM"));
        assert!(scheme_allowed("mailto:a@b.c"));
        assert!(!scheme_allowed("file:///C:/x"));
        assert!(!scheme_allowed("javascript:alert(1)"));
        assert!(!scheme_allowed("ftp://a.com"));
    }

    #[test]
    fn executables_are_blocked() {
        assert!(ext_blocked(Path::new("C:\\x\\a.exe")));
        assert!(ext_blocked(Path::new("C:\\x\\A.BAT")));
        assert!(ext_blocked(Path::new("C:\\x\\run.ps1")));
        assert!(!ext_blocked(Path::new("C:\\x\\报告.pdf")));
        assert!(!ext_blocked(Path::new("C:\\x\\out.mp4")));
        // 无扩展名（目录常见）不拦，交给 open_path 自己处理
        assert!(!ext_blocked(Path::new("C:\\x\\build")));
    }
}
