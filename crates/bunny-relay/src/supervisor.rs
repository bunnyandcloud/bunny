use bunny_core::types::RestartPolicy;
use bunny_core::types::{BrowserStatus, SessionStatus, TerminalStatus};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct ComponentHealth {
    pub restart_count: u32,
    pub last_restart: Option<Instant>,
    pub status: String,
}

pub struct ProcessSupervisor {
    pub max_restarts: u32,
    pub restart_window: Duration,
    components: HashMap<String, ComponentHealth>,
}

impl ProcessSupervisor {
    pub fn new(max_restarts: u32, restart_window_seconds: u64) -> Self {
        Self {
            max_restarts,
            restart_window: Duration::from_secs(restart_window_seconds),
            components: HashMap::new(),
        }
    }

    pub fn record_crash(&mut self, component: &str) -> bool {
        let health = self
            .components
            .entry(component.to_string())
            .or_insert(ComponentHealth {
                restart_count: 0,
                last_restart: None,
                status: "unknown".into(),
            });

        let now = Instant::now();
        if let Some(last) = health.last_restart {
            if now.duration_since(last) > self.restart_window {
                health.restart_count = 0;
            }
        }
        health.restart_count += 1;
        health.last_restart = Some(now);
        health.restart_count <= self.max_restarts
    }

    pub fn should_restart(&self, policy: RestartPolicy, component: &str) -> bool {
        match policy {
            RestartPolicy::Never | RestartPolicy::Manual => false,
            RestartPolicy::Always => true,
            RestartPolicy::OnFailure => self
                .components
                .get(component)
                .map(|h| h.restart_count <= self.max_restarts)
                .unwrap_or(true),
        }
    }

    pub fn session_status_from_components(
        &self,
        terminal: TerminalStatus,
        browser: BrowserStatus,
    ) -> SessionStatus {
        match (terminal, browser) {
            (TerminalStatus::Running, BrowserStatus::Running) => SessionStatus::Ready,
            (TerminalStatus::Crashed, _) | (_, BrowserStatus::Crashed) => SessionStatus::Degraded,
            (TerminalStatus::Reconnectable, _) | (_, BrowserStatus::Reconnectable) => {
                SessionStatus::Recoverable
            }
            (_, BrowserStatus::ResetRequired) => SessionStatus::Recoverable,
            (TerminalStatus::Stopped, BrowserStatus::Stopped) => SessionStatus::Stopped,
            _ => SessionStatus::Degraded,
        }
    }
}
