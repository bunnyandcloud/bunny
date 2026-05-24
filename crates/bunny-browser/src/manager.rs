use crate::stack::{BrowserStack, BrowserStackConfig};
use anyhow::Result;
use bunny_core::types::BrowserStatus;
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

pub struct BrowserManager {
    sessions: RwLock<HashMap<Uuid, BrowserStack>>,
    default_width: u32,
    default_height: u32,
}

impl BrowserManager {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            default_width: width,
            default_height: height,
        }
    }

    pub fn create(&self, stream_session_id: Uuid, target_url: &str) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let profile_dir = format!("/tmp/bunny-chromium-{id}");
        let stack = BrowserStack::start(&BrowserStackConfig {
            width: self.default_width,
            height: self.default_height,
            target_url: target_url.to_string(),
            ephemeral_profile: false,
            profile_dir: Some(profile_dir),
        })?;
        let _ = stream_session_id;
        self.sessions.write().insert(id, stack);
        Ok(id)
    }

    pub fn get_novnc_port(&self, id: Uuid) -> Option<u16> {
        self.sessions.read().get(&id).map(|s| s.novnc_port)
    }

    pub fn get_cdp_port(&self, id: Uuid) -> Option<u16> {
        self.sessions.read().get(&id).map(|s| s.cdp_port)
    }

    pub fn restart(&self, id: Uuid, target_url: &str) -> Result<()> {
        if let Some(mut stack) = self.sessions.write().remove(&id) {
            stack.stop();
        }
        let profile_dir = format!("/tmp/bunny-chromium-{id}");
        let stack = BrowserStack::start(&BrowserStackConfig {
            width: self.default_width,
            height: self.default_height,
            target_url: target_url.to_string(),
            ephemeral_profile: false,
            profile_dir: Some(profile_dir),
        })?;
        self.sessions.write().insert(id, stack);
        Ok(())
    }

    pub fn stop(&self, id: Uuid) {
        if let Some(mut stack) = self.sessions.write().remove(&id) {
            stack.stop();
        }
    }

    pub fn health_check(&self) {
        let mut sessions = self.sessions.write();
        for stack in sessions.values_mut() {
            if !stack.is_running() {
                stack.status = BrowserStatus::Crashed;
            }
        }
    }
}
