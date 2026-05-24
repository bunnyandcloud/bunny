use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalClientMsg {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
    Ping { id: u64 },
    Subscribe { from_offset: Option<u64> },
    Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalServerMsg {
    Output { data: String, offset: u64 },
    Replay {
        chunks: Vec<ReplayChunk>,
        /// When true, client should keep replay visible (skip tmux full-screen refresh).
        #[serde(default)]
        has_history: bool,
    },
    Status { status: String, exit_code: Option<i32> },
    Error { code: String, message: String },
    Pong { id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayChunk {
    pub offset: u64,
    pub data: String,
}
