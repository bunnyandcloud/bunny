use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalClientMsg {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
    Ping { id: u64 },
    Subscribe {
        from_offset: Option<u64>,
        #[serde(default)]
        live_attach: bool,
        /// Notebook attach drawer: record submitted lines as blocks without re-executing.
        #[serde(default)]
        notebook_record: bool,
    },
    Refresh,
    VisibleSnapshot,
    BlocksSubscribe { from_seq: Option<i64> },
    CommandSubmit { text: String },
    /// User pressed Enter in the notebook attach TTY (command already sent as `input`).
    TtyCommandRecord { text: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReplayMode {
    #[default]
    None,
    CatchUp,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalServerMsg {
    Output { data: String, offset: u64 },
    Replay {
        chunks: Vec<ReplayChunk>,
        replay_mode: ReplayMode,
        snapshot_offset: u64,
        /// Legacy alias: true when `replay_mode == Recovery`.
        #[serde(default)]
        has_history: bool,
    },
    Status { status: String, exit_code: Option<i32> },
    Error { code: String, message: String },
    Pong { id: u64 },
    /// Current visible tmux pane (notebook interactive embed).
    PaneSnapshot { data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayChunk {
    pub offset: u64,
    pub data: String,
}

/// Live PTY output tagged with the buffer line offset at end-of-chunk.
#[derive(Debug, Clone)]
pub struct TerminalOutput {
    pub offset: u64,
    pub data: String,
}
