use crate::types::PushMessage;
use anyhow::Result;
use tracing::{debug, warn};

/// FCM legacy HTTP API (server key). Set `BUNNY_FCM_SERVER_KEY` or pass key to `new`.
#[derive(Clone)]
pub struct FcmClient {
    server_key: Option<String>,
    http: reqwest::Client,
}

impl FcmClient {
    pub fn from_env() -> Self {
        Self {
            server_key: std::env::var("BUNNY_FCM_SERVER_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            http: reqwest::Client::new(),
        }
    }

    pub fn new(server_key: Option<String>) -> Self {
        Self {
            server_key,
            http: reqwest::Client::new(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.server_key.is_some()
    }

    pub async fn send_to_token(&self, token: &str, message: &PushMessage) -> Result<bool> {
        let Some(key) = &self.server_key else {
            debug!("FCM not configured — skipping push");
            return Ok(false);
        };

        let mut payload = serde_json::json!({
            "to": token,
            "priority": "high",
            "notification": {
                "title": message.title,
                "body": message.body,
            },
        });
        if let Some(data) = &message.data {
            payload["data"] = serde_json::Value::Object(data.clone());
        }

        let res = self
            .http
            .post("https://fcm.googleapis.com/fcm/send")
            .header("Authorization", format!("key={key}"))
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            warn!(%body, "FCM send failed");
            return Ok(false);
        }
        Ok(true)
    }
}
