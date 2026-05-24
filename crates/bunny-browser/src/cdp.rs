use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserEvent {
    Console {
        level: String,
        text: String,
        url: Option<String>,
        ts: String,
    },
    NetworkStarted {
        request_id: String,
        method: String,
        url_redacted: String,
        resource_type: String,
        ts: String,
    },
    NetworkCompleted {
        request_id: String,
        status: u16,
        timing_ms: Option<f64>,
        size: Option<u64>,
        ts: String,
    },
    NetworkFailed {
        request_id: String,
        error: String,
        ts: String,
    },
    Navigation {
        url_redacted: String,
        ts: String,
    },
    PageError {
        message: String,
        ts: String,
    },
    Screenshot {
        ref_id: String,
        ts: String,
    },
}

/// CDP collector runs as optional Node sidecar (see apps/server/cdp-sidecar).
pub struct CdpCollectorConfig {
    pub cdp_url: String,
    pub session_id: String,
}
