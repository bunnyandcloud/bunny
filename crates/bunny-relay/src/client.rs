use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

pub struct RelayClient {
    pub relay_url: String,
    pub agent_id: String,
    pub backoff_initial_ms: u64,
    pub backoff_max_ms: u64,
}

impl RelayClient {
    pub fn new(relay_url: String, agent_id: String) -> Self {
        Self {
            relay_url,
            agent_id,
            backoff_initial_ms: 1000,
            backoff_max_ms: 60000,
        }
    }

    pub async fn connect_loop(&self) {
        let mut backoff = self.backoff_initial_ms;
        loop {
            match self.try_connect().await {
                Ok(()) => {
                    backoff = self.backoff_initial_ms;
                }
                Err(e) => {
                    warn!(error = %e, backoff_ms = backoff, "relay connection failed");
                    sleep(Duration::from_millis(backoff)).await;
                    backoff = (backoff * 2).min(self.backoff_max_ms);
                }
            }
        }
    }

    async fn try_connect(&self) -> Result<()> {
        info!(url = %self.relay_url, agent = %self.agent_id, "connecting to relay");
        // MVP: placeholder — full WSS outbound tunnel in production
        sleep(Duration::from_secs(30)).await;
        Err(anyhow::anyhow!("relay not configured"))
    }
}
