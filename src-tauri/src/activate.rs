//! 激活码入网：把「找管理员抄 48 位 hex」换成「输 8 位激活码」。
//!
//! 契约见 docs/terminal-routing.md §4.1（乙）：
//! ```text
//! POST {server}/api/terminals/activate
//! { "code": "7F3A-9C21", "alias": "书房 Windows", "device_fp": "…", "os": "windows", "app_version": "0.1.0" }
//! → 200 { "terminal": { "id": "term_…", "alias": "…", "labels": ["办公"] }, "token": "net_…" }  // token 只此一次返回
//! → 409 { "error": "code_used" }
//! ```
//!
//! 激活成功后 token 直接写进设置并缓存 terminal_id——用户不需要见到那串
//! hex，下一步点「连接」就是已授权状态。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings;

#[derive(Debug, Deserialize)]
struct ActivateResponse {
    terminal: TerminalBody,
    token: String,
}

#[derive(Debug, Deserialize)]
struct TerminalBody {
    id: String,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    labels: Option<Vec<String>>,
}

/// REST 基址从 WS 地址推导：`wss://host:8443/ws` → `https://host:8443`。
/// 用户只配置过一个服务端地址，激活入口不应该让他再填一遍。
fn rest_base(ws_url: &str) -> Option<String> {
    let t = ws_url.trim();
    let (scheme, rest) = if let Some(r) = t.strip_prefix("wss://") {
        ("https", r)
    } else if let Some(r) = t.strip_prefix("ws://") {
        ("http", r)
    } else if let Some(r) = t.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = t.strip_prefix("http://") {
        ("http", r)
    } else {
        // 裸地址（host:port）也接：conn::normalize_url 会补 ws://，这里对称补 http
        ("http", t)
    };
    let host = rest.split('/').next()?.trim().to_string();
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Activated {
    pub token: String,
    pub terminal_id: String,
    pub alias: String,
    pub labels: Vec<String>,
    /// v2 登记：服务端标识符，客户端 pin 住防串服务器（close 4004）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    /// v2 登记：本地自生成的设备标识符（轮换/重新登记时复用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_token: Option<String>,
    /// 走的哪条登记路径（前端提示用）
    pub via: String,
}

/// 本地自生成 32 位 hex 设备标识符（16 字节随机）。
/// 秘密在生成它的机器上，登记只把 sha256 交给服务端。
fn gen_device_token() -> String {
    uuid::Uuid::new_v4()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn http_agent() -> Result<ureq::Agent, String> {
    // 与 media.rs 同一 TLS 策略：内网自签证书是常态，校验关掉。
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(Duration::from_secs(20))
        .build())
}

fn read_body(resp: ureq::Response) -> Result<String, String> {
    resp.into_reader()
        .take(64 * 1024)
        .bytes()
        .collect::<Result<Vec<u8>, _>>()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| format!("响应读不出来: {e}"))
}

/// 用激活码/邀请码入网，成功即写回设置。
///
/// v2 优先：本地生成 device_token 走 `/network/register-v2`（登记制，
/// 服务端只存 hash）；端点不存在（404）自动降级 v1 `/api/terminals/activate`
/// （签发制）。两条路的终点都是「令牌写进设置、自动连接」。
///
/// Err 的字符串直接展示给用户：这里是「第一次接触」的流程，
/// 说行人话比技术黑话重要——错误里告诉他下一步该干什么。
#[tauri::command]
pub async fn terminal_activate(
    app: AppHandle,
    code: String,
    alias: Option<String>,
) -> Result<Activated, String> {
    let code = code.trim().to_string();
    if code.is_empty() {
        return Err("请输入激活码（找服务端管理员申请）".into());
    }

    let s = settings::current(&app);
    if s.server_url.trim().is_empty() {
        return Err("先在上面填好服务端地址，再输入激活码".into());
    }
    let base = rest_base(&s.server_url)
        .ok_or_else(|| "服务端地址不合法，请检查后重试".to_string())?;

    let dev = crate::device::load_or_create(&app);
    let resolved_alias = alias
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| dev.hostname.clone());
    let base = base;

    let out = tauri::async_runtime::spawn_blocking(move || -> Result<Activated, String> {
        let agent = http_agent()?;

        // ── v2 登记（优先）：device_token 本地生成，服务端只存 hash ──
        let device_token = gen_device_token();
        let v2_body = serde_json::json!({
            "device_token": device_token,
            "invite": code,
            "alias": resolved_alias,
            "device": {
                "device_fp": dev.device_id,
                "os": dev.os,
                "app_version": dev.app_version,
            },
        });
        let v2 = agent
            .post(&format!("{base}/network/register-v2"))
            .set("content-type", "application/json")
            .send_string(&v2_body.to_string());

        match v2 {
            Ok(resp) => {
                let raw = read_body(resp)?;
                let parsed: serde_json::Value = serde_json::from_slice(raw.as_bytes())
                    .map_err(|e| format!("登记成功，但响应解析失败: {e}"))?;
                let token = parsed
                    .get("token")
                    .and_then(|v| v.as_str())
                    .filter(|t| !t.is_empty())
                    .ok_or("登记响应里没有 token")?
                    .to_string();
                let terminal_id = parsed
                    .get("terminal_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("term-unknown")
                    .to_string();
                let server_id = parsed
                    .get("server_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return Ok(Activated {
                    token,
                    terminal_id,
                    alias: resolved_alias,
                    labels: vec![],
                    server_id,
                    device_token: Some(device_token),
                    via: "v2".into(),
                });
            }
            Err(ureq::Error::Status(404, _)) => {
                // 端点不存在 → 老服务端，降级 v1 签发制
            }
            Err(ureq::Error::Status(409, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(format!(
                    "这个激活码已被使用（一个码只能激活一台终端），请申请新码{}",
                    if body.is_empty() { String::new() } else { format!("（{body}）") }
                ));
            }
            Err(ureq::Error::Status(410, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(format!(
                    "激活码已过期（10 分钟有效期），请申请新码{}",
                    if body.is_empty() { String::new() } else { format!("（{body}）") }
                ));
            }
            Err(ureq::Error::Status(c, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(format!(
                    "服务端返回 {c}: {}",
                    body.chars().take(120).collect::<String>()
                ));
            }
            Err(e) => return Err(format!("连不上服务端（地址对了吗？）: {e}")),
        }

        // ── v1 兜底：/api/terminals/activate（签发制，老服务端）──
        let body = serde_json::json!({
            "code": code,
            "alias": resolved_alias,
            "device_fp": dev.device_id,
            "os": dev.os,
            "app_version": dev.app_version,
        });
        let resp = agent
            .post(&format!("{base}/api/terminals/activate"))
            .set("content-type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| match e {
                ureq::Error::Status(409, _) => {
                    "这个激活码已被使用（一个码只能激活一台终端），请申请新码".to_string()
                }
                ureq::Error::Status(410, _) => {
                    "激活码已过期（10 分钟有效期），请申请新码".to_string()
                }
                ureq::Error::Status(400, r) => {
                    let body = r.into_string().unwrap_or_default();
                    format!("服务端没接受这个激活码: {}", body.chars().take(120).collect::<String>())
                }
                ureq::Error::Status(404, _) => {
                    "服务端还没有激活接口（旧版本？），请让服务端升级后再试".to_string()
                }
                ureq::Error::Status(c, _) => format!("服务端返回 {c}，激活码或地址可能不对"),
                _ => format!("连不上服务端（地址对了吗？）: {e}"),
            })?;

        let raw = read_body(resp)?;
        let parsed: ActivateResponse = serde_json::from_slice(raw.as_bytes())
            .map_err(|e| format!("激活成功了，但响应解析失败: {e}"))?;

        Ok(Activated {
            token: parsed.token,
            terminal_id: parsed.terminal.id,
            alias: parsed.terminal.alias.unwrap_or_default(),
            labels: parsed.terminal.labels.unwrap_or_default(),
            server_id: None,
            device_token: None,
            via: "v1".into(),
        })
    })
    .await
    .map_err(|e| format!("激活任务中断: {e}"))??;

    // 成功：token 三处同步（内存态 + 磁盘 + 全窗口广播），与 settings_set
    // 同语义。只写磁盘的坑：conn_connect 读的是内存态快照，旧 token 会让
    // 重连仍以旧身份握手，激活看起来「没生效」。
    let mut s = settings::current(&app);
    s.token = out.token.clone();
    s.server_id = out.server_id.clone();
    s.device_token = out.device_token.clone();
    if let Some(state) = app.try_state::<settings::SettingsState>() {
        if let Ok(mut g) = state.0.lock() {
            *g = s.clone();
        }
    }
    settings::save(&app, &s)?;
    let _ = app.emit("ec:settings-updated", &s);
    settings::cache_terminal_id(&app, &out.terminal_id);

    Ok(out)
}

#[derive(Debug, Clone, Serialize, Default, Deserialize)]
pub struct ServerInfo {
    /// 服务端自称的名字（如「公司通知网关」）
    #[serde(default)]
    pub name: Option<String>,
    /// 入网指引链接（自助页/文档/表单——形态由部署方决定，客户端只展示）
    #[serde(default)]
    pub join_doc: Option<String>,
    /// 联系方式文案（没有自助页时，告诉用户找谁）
    #[serde(default)]
    pub contact: Option<String>,
}

/// 拉服务端自述的入网指引（可选能力）。
///
/// 解耦原则：令牌怎么签发、怎么送达用户，是部署方的内部事务。客户端不
/// 假设任何发放流程，只认这一个纯展示性的端点——内容部署方全权自定义。
/// 拉不到（404 / 网络错 / 老服务端）一律返回 Ok(None)，界面显示通用文案。
#[tauri::command]
pub async fn server_info(app: AppHandle) -> Result<Option<ServerInfo>, String> {
    let s = settings::current(&app);
    if s.server_url.trim().is_empty() {
        return Ok(None);
    }
    let Some(base) = rest_base(&s.server_url) else {
        return Ok(None);
    };

    tauri::async_runtime::spawn_blocking(move || -> Option<ServerInfo> {
        let tls = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .ok()?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .timeout(Duration::from_secs(5))
            .build();
        let resp = agent
            .get(&format!("{base}/api/server-info"))
            .call()
            .ok()?;
        let raw = resp
            .into_reader()
            .take(16 * 1024)
            .bytes()
            .collect::<Result<Vec<u8>, _>>()
            .ok()?;
        let info: ServerInfo = serde_json::from_slice(&raw).ok()?;
        let has_any = info.name.as_deref().or(info.join_doc.as_deref()).or(info.contact.as_deref()).is_some_and(|v| !v.trim().is_empty());
        has_any.then_some(info)
    })
    .await
    .map_err(|e| format!("查询中断: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_base_from_ws_variants() {
        assert_eq!(
            rest_base("wss://bridge.example:8443/ws"),
            Some("https://bridge.example:8443".into())
        );
        assert_eq!(
            rest_base("ws://127.0.0.1:8787/ws"),
            Some("http://127.0.0.1:8787".into())
        );
        assert_eq!(
            rest_base("https://orch.example:8443"),
            Some("https://orch.example:8443".into())
        );
        // 裸地址当 http 对称补全
        assert_eq!(rest_base("192.168.1.5:8791"), Some("http://192.168.1.5:8791".into()));
        // 只有路径没有 host → None（调用方提示检查地址）
        assert_eq!(rest_base("///ws"), None);
    }
}
