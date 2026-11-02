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

/// 允许交给系统打开的文档/媒体扩展名（白名单）。
///
/// 为什么从黑名单改成白名单（工单 #9）：黑名单挡得住直球，挡不住变形——
/// `evil.exe.`（尾点）在 Windows 打开时被规约成 `evil.exe`，但扩展名检查
/// 看到的是 `exe.`；`.url` / `.cpl` / `.msc` / `.scf` 这类「不是 exe 却能
/// 执行代码」的格式还会随系统能力不断长出来。白名单 fail-closed：
/// 不在名单里的一律拒绝，新出现的危险格式天然进不来。
const ALLOWED_EXTS: &[&str] = &[
    // 图片
    "png", "jpg", "jpeg", "gif", "webp", "bmp",
    // 音视频
    "mp3", "wav", "ogg", "flac", "m4a", "mp4", "webm", "mkv", "mov", "avi",
    // 文档
    "pdf", "txt", "md", "csv", "tsv", "log", "json", "xml", "yaml", "yml",
    "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    // 压缩包只读不解执行
    "zip", "tar", "gz", "7z",
    // 设计/工程只读格式
    "psd", "ai", "fig", "sqlite3", "db",
];

/// 绝不代开的扩展名。白名单已把它们全部挡在外面，这里单独列出
/// 是给读代码的人一张「曾经担心过什么」的清单，也做第二道闸。
const BLOCKED_EXTS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "pif", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf",
    "wsh", "hta", "msi", "msp", "mst", "jar", "lnk", "reg", "gadget", "iso", "img",
    // 不是 exe 却能执行/加载代码的格式（工单 #9 补）
    "url", "cpl", "msc", "scf", "inf", "sct", "ws", "shb", "shs", "msh", "py", "pyw", "rb",
    "pl", "sh", "bash", "fish",
    // 本地 HTML 能带脚本跑，服务端下发的页面不该在 file:// 里活起来
    "html", "htm", "xhtml", "shtml", "mht",
];

fn scheme_allowed(s: &str) -> bool {
    let l = s.to_lowercase();
    URL_PREFIXES.iter().any(|p| l.starts_with(p))
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.trim_end_matches(['.', ' ']).to_lowercase())
        .unwrap_or_default()
}

fn ext_blocked(p: &Path) -> bool {
    let e = ext_of(p);
    BLOCKED_EXTS.iter().any(|b| *b == e)
}

/// 白名单判定：目录（无扩展名）不算可打开文档，一律拒绝。
pub fn ext_allowed(p: &Path) -> bool {
    let e = ext_of(p);
    !e.is_empty() && ALLOWED_EXTS.iter().any(|a| *a == e)
}

/// 文档白名单 + 可执行黑名单的合并判定：sys_open 与 media_open 共用。
/// 工单 #9 的「三处汇合点」收成一道闸：任何一个入口想用系统程序
/// 打开本机文件，都必须过这里。
pub fn is_openable_document(p: &Path) -> bool {
    !ext_blocked(p) && ext_allowed(p)
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

    let raw = std::path::PathBuf::from(&t);
    if !raw.is_absolute() {
        return Err("只支持 http/https 链接或本机绝对路径".into());
    }
    if !raw.exists() {
        return Err("这个文件不在这台机器上，打不开".into());
    }

    // 先规约再校验再打开，三步都用同一条规约路径（工单 #9）：
    // 1. `evil.exe.` 这类尾点/尾空格写法，Windows 打开时会被规约成
    //    `evil.exe`——校验原始串看到的是 `exe.`，骗过黑名单；
    //    canonicalize 把它落成真实路径，扩展名检查就没得骗。
    // 2. 后面 open_path 也必须打开规约后的路径，否则校验的和打开的
    //    不是同一个东西。
    let target = std::fs::canonicalize(&raw).map_err(|e| format!("系统打不开这个文件: {e}"))?;

    if ext_blocked(&target) {
        return Err("可执行文件不允许直接打开".into());
    }
    if !ext_allowed(&target) {
        return Err("这个类型的文件不允许代开（只放行常见文档与媒体格式）".into());
    }
    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<&str>)
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

    /// 工单 #9：黑名单变形绕过的三类代表，白名单必须全部拒绝。
    #[test]
    fn allowlist_closes_bypass_surfaces() {
        // 尾点 / 尾空格：`evil.exe.` 规约后是 exe，且 `exe.` 本就不在白名单
        assert!(!ext_allowed(Path::new("C:\\x\\evil.exe.")));
        assert!(!ext_allowed(Path::new("C:\\x\\evil.exe ")));
        // 「不是 exe 却能执行」的格式
        assert!(!ext_allowed(Path::new("C:\\x\\payload.url")));
        assert!(!ext_allowed(Path::new("C:\\x\\panel.cpl")));
        assert!(!ext_allowed(Path::new("C:\\x\\console.msc")));
        assert!(!ext_allowed(Path::new("C:\\x\\page.html")));
        assert!(!ext_allowed(Path::new("C:\\x\\run.py")));
        // 目录 / 无扩展名一律拒绝（fail-closed）
        assert!(!ext_allowed(Path::new("C:\\x\\build")));
        assert!(!ext_allowed(Path::new("C:\\x\\noext")));
        // 正常文档/媒体放行
        assert!(ext_allowed(Path::new("C:\\x\\报告.PDF")));
        assert!(ext_allowed(Path::new("C:\\x\\out.mp4")));
        assert!(ext_allowed(Path::new("C:\\x\\数据.xlsx")));
        // 尾点写法的原始串语义：`report.pdf.` 的扩展名提取为空（Rust 把尾点
        // 之后当空扩展）→ 白名单拒绝。真实闸门跑在 canonicalize 之后，Windows
        // 会先把尾点规约掉（`report.pdf.` → `report.pdf`），正常文件不受影响；
        // 这里断言的是「就算绕过规约也进不来」的 fail-closed 底线。
        assert_eq!(ext_of(Path::new("C:\\x\\report.pdf.")), "");
        assert!(!ext_allowed(Path::new("C:\\x\\report.pdf.")));
    }
}
