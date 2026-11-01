// 这些结构目前只有定义、还没有调用方：连接管理与通知推送会在下一步接线。
// 用模块级 allow 而不是逐个标注，是为了让真正的死代码仍然能被看见。
#![allow(dead_code)]

use crate::models::EchoMessage;
use serde::{Deserialize, Serialize};

/// EchoCast 节点与服务端之间的线路帧（JSON 文本帧）。
///
/// ⚠️ 约定优先：当前版本是按最通用的聊天协议草案实现的，
/// 等真实服务端接口文档到位后，只需要修改本文件里的
/// `WireFrame::message` / `WireFrame::hello` / `into_message`，
/// 上层的连接管理与 UI 均无需改动。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFrame {
    /// 帧类型："msg" | "hello" | "ack" | "err" | "ping" | "pong"
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

impl WireFrame {
    /// 把一条消息打包成发送帧
    pub fn message(m: &EchoMessage) -> Self {
        Self {
            op: "msg".into(),
            id: Some(m.id.clone()),
            channel: Some(m.channel.clone()),
            sender: Some(m.sender.clone()),
            content: Some(m.content.clone()),
            timestamp: Some(m.timestamp),
        }
    }

    /// 连接建立后的报到帧
    pub fn hello(node: &str, channel: &str) -> Self {
        Self {
            op: "hello".into(),
            channel: Some(channel.to_string()),
            sender: Some(node.to_string()),
            id: None,
            content: None,
            timestamp: Some(EchoMessage::now_ms()),
        }
    }

    pub fn ping() -> Self {
        Self {
            op: "ping".into(),
            id: None,
            channel: None,
            sender: None,
            content: None,
            timestamp: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    /// 将 "msg" 帧还原为 EchoMessage；缺失字段给出默认值
    pub fn into_message(self) -> Option<EchoMessage> {
        if self.op != "msg" {
            return None;
        }
        Some(EchoMessage {
            id: self
                .id
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            channel: self
                .channel
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "general".into()),
            sender: self.sender.unwrap_or_else(|| "unknown".into()),
            content: self.content.unwrap_or_default(),
            timestamp: self.timestamp.unwrap_or_else(EchoMessage::now_ms),
        })
    }
}
