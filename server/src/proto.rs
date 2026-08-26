use serde::{Deserialize, Serialize};

/// All messages exchanged over the WebSocket, JSON-encoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    Hello {
        pin: String,
        name: String,
    },
    Welcome {
        ok: bool,
        err: Option<String>,
    },
    Text {
        id: String,
        body: String,
        ts: u64,
    },
    Img {
        id: String,
        name: String,
        mime: String,
        data: String,
        ts: u64,
    },
    Ack {
        id: String,
        ok: bool,
        err: Option<String>,
    },
    Ping,
    Pong,
}

pub fn to_json(m: &Msg) -> String {
    serde_json::to_string(m).unwrap_or_default()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    format!("{:x}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0))
}
