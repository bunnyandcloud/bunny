use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

/// Signals active watch noVNC proxies to disconnect when a link is revoked.
#[derive(Default)]
pub struct WatchHub {
    revoke: RwLock<HashMap<String, watch::Sender<bool>>>,
}

impl WatchHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn revoke_tx(&self, token: &str) -> watch::Sender<bool> {
        let mut map = self.revoke.write();
        map.entry(token.to_string())
            .or_insert_with(|| watch::channel(false).0)
            .clone()
    }

    /// Subscribe before upgrading the WebSocket; `true` means the link was stopped.
    pub fn subscribe(&self, token: &str) -> watch::Receiver<bool> {
        self.revoke_tx(token).subscribe()
    }

    pub fn revoke(&self, token: &str) {
        if let Some(tx) = self.revoke.read().get(token) {
            let _ = tx.send(true);
        }
    }

    pub fn revoke_many(&self, tokens: &[String]) {
        for token in tokens {
            self.revoke(token);
        }
    }

    /// Shared flag + task that flips it when the watch link is revoked mid-session.
    pub fn revoked_flag(mut revoked_rx: watch::Receiver<bool>) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(*revoked_rx.borrow()));
        if flag.load(Ordering::SeqCst) {
            return flag;
        }
        let flag_task = flag.clone();
        tokio::spawn(async move {
            while revoked_rx.changed().await.is_ok() {
                if *revoked_rx.borrow() {
                    flag_task.store(true, Ordering::SeqCst);
                    break;
                }
            }
        });
        flag
    }
}
