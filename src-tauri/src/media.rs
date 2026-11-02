// 媒体附件的下载与缓存。
//
// 下载放在 Rust 侧而不是 WebView 里做，三个原因：
// 1. 跨域 CORS 只管浏览器上下文，Rust 请求不受它牵制 —— 媒体服务几乎
//    一定和 WS 不同源，靠服务端配 CORS 头是把命运交到别人手里；
// 2. 自签证书只在 Rust 侧有「接受」的开关，与 conn.rs 的 wss 同一套策略；
// 3. 缓存下来的本地文件走 asset 协议交给 WebView，<img>/<video>/<audio>
//    直接可播，刷新、重开窗口都不用重新下载。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use native_tls::TlsConnector;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use ureq::AgentBuilder;

/// 单文件下载上限。服务端还没给大小约定（信已发），先按 200MB 兜底，
/// 超限直接拒绝而不是把磁盘写满。
const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// 下载结果。path 是本地缓存文件的绝对路径，前端用 convertFileSrc 转成 asset:// 再播。
#[derive(Debug, Serialize)]
pub struct MediaFetched {
    pub path: String,
    pub size: u64,
    /// 命中缓存 = 没走网络。前端可以用来区分「秒开」和「刚下完」。
    pub cached: bool,
}

/// FNV-1a 64。缓存文件名 = URL 指纹 + 扩展名：同一个 URL 永远同一个文件，
/// 冲突概率与 sha1 同阶但省一个依赖（这里只做缓存键，不做安全用途）。
fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("media-cache");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// 扩展名：先信 URL，再按 mime 查表，都不行就 bin。
fn ext_of(url: &str, mime: Option<&str>) -> String {
    let path = url.split('?').next().unwrap_or(url);
    if let Some(dot) = path.rfind('.') {
        let e = &path[dot + 1..];
        // 扩展名合理的样子：1–5 个字母数字。太长或带怪字符的一律不信
        if (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()) {
            return e.to_lowercase();
        }
    }
    let m = mime.unwrap_or("").to_lowercase();
    let table: &[(&str, &str)] = &[
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
        ("audio/mpeg", "mp3"),
        ("audio/wav", "wav"),
        ("audio/ogg", "ogg"),
        ("video/mp4", "mp4"),
        ("video/webm", "webm"),
        ("application/pdf", "pdf"),
        ("application/zip", "zip"),
        ("text/csv", "csv"),
    ];
    for (k, v) in table {
        if m.contains(k) {
            return (*v).into();
        }
    }
    "bin".into()
}

/// 校验 + 查缓存 + 下载。命令和预取共用这一条路。
fn ensure_cached(
    app: AppHandle,
    url: String,
    token: Option<String>,
) -> Result<MediaFetched, String> {
    let lower = url.to_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("只支持 http/https 的媒体地址".into());
    }

    let dir = cache_dir(&app)?;
    let dest = dir.join(format!("{:016x}{}", fnv64(&url), ext_of(&url, None)));

    if let Ok(meta) = std::fs::metadata(&dest) {
        if meta.len() > 0 {
            return Ok(MediaFetched {
                path: dest.to_string_lossy().into_owned(),
                size: meta.len(),
                cached: true,
            });
        }
    }

    let (path, size) = download(&url, token, &dest)?;
    Ok(MediaFetched {
        path,
        size,
        cached: false,
    })
}

/// 拉取一个媒体附件到本地缓存。
///
/// token 由前端从设置里带来（连接用的同一个）。缓存键是 URL ——
/// 同 URL 只下一次，签名过期的 URL 会算出新键重新下载，行为正好正确。
#[tauri::command]
pub async fn media_fetch(
    app: AppHandle,
    url: String,
    token: Option<String>,
) -> Result<MediaFetched, String> {
    tauri::async_runtime::spawn_blocking(move || ensure_cached(app, url, token))
        .await
        .map_err(|e| format!("下载任务中断: {e}"))?
}

/// 通知到达时预取全部附件。串行逐个下，失败静默 ——
/// 面板里还有重试按钮，后台下载失败不该变成第二条通知去吵用户。
///
/// 为什么要预取：点开面板才开始下的话，图片要看着它一块块加载，
/// 大文件更是点开就转圈。「传文件」的预期是通知到了文件就在本地了。
pub fn prefetch(app: &AppHandle, media: &[crate::models::MediaItem], token: Option<String>) {
    if media.is_empty() {
        return;
    }
    let app = app.clone();
    let items: Vec<crate::models::MediaItem> = media.to_vec();
    tauri::async_runtime::spawn(async move {
        for m in items {
            let a = app.clone();
            let t = token.clone();
            let url = m.url.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                ensure_cached(a, url, t).map(|f| f.size)
            })
            .await;
        }
    });
}

/// 阻塞式下载，在 spawn_blocking 里跑。先写 .part 再改名：
/// 写一半断电/崩溃也不会把坏文件留在缓存里冒充好文件。
fn download(url: &str, token: Option<String>, dest: &Path) -> Result<(String, u64), String> {
    // 与 conn.rs 的 wss 同一策略：内网自签证书，关校验。
    // 这里只拉媒体文件，不执行不解析，风险面与「用户自己用浏览器打开」相同。
    let tls = TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let agent = AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(Duration::from_secs(300))
        .build();

    let req = agent.get(url);
    let req = match &token {
        Some(t) => req.set("Authorization", &format!("Bearer {t}")),
        None => req,
    };
    let resp = req.call().map_err(|e| format!("下载失败: {e}"))?;

    if let Some(len) = resp.header("Content-Length").and_then(|v| v.parse::<u64>().ok()) {
        if len > MAX_BYTES {
            return Err(format!("文件超过下载上限 {} MB", MAX_BYTES / 1024 / 1024));
        }
    }

    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err("文件超过下载上限".into());
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    drop(file);
    std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
    Ok((dest.to_string_lossy().into_owned(), total))
}

/// 用系统默认程序打开一个已下载的媒体文件。
///
/// 白名单只放行 media-cache 目录内的路径：服务端推来的 url 归根到底
/// 是远程输入，绝不能变成「在用户机器上打开任意路径」的入口。
#[tauri::command]
pub async fn media_open(app: AppHandle, path: String) -> Result<(), String> {
    let dir = cache_dir(&app)?;
    let dir = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;
    let target = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
    if !target.starts_with(&dir) {
        return Err("只允许打开媒体缓存目录里的文件".into());
    }
    // 目录白名单之外还要过文档白名单（工单 #9 的第二处汇合点）：
    // media[] 的 URL 是服务端可控的，`.exe` 结尾的「附件」下载进缓存后
    // 若直接交给系统程序，等于远程执行。缓存文件名保留原始扩展名，
    // 这里能准确判型。
    if !crate::sysopen::is_openable_document(&target) {
        return Err("这个类型的附件不允许代开".into());
    }
    app.opener()
        .open_path(target.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_prefers_url_then_mime() {
        assert_eq!(ext_of("https://a/b/c.PNG?sig=1", None), "png");
        assert_eq!(ext_of("https://a/b/noext", Some("audio/mpeg")), "mp3");
        assert_eq!(ext_of("https://a/b/noext", Some("application/pdf")), "pdf");
        assert_eq!(ext_of("https://a/b/noext", None), "bin");
        // 假扩展名（太长）不信，落到 mime
        assert_eq!(ext_of("https://a/b/x.eviljs", Some("video/mp4")), "mp4");
    }

    #[test]
    fn fnv_is_stable_per_url() {
        assert_eq!(fnv64("https://a/x.png"), fnv64("https://a/x.png"));
        assert_ne!(fnv64("https://a/x.png"), fnv64("https://a/x.png?sig=2"));
    }
}
