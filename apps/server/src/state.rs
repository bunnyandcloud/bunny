use crate::cdp_collector::CdpCollectorHandle;
use crate::claude::{AuthFlow, InstallState};
use crate::realtime::RealtimeHub;
use crate::watch_hub::WatchHub;
use anyhow::Result;
use bunny_auth::AuthService;
use bunny_discord::DiscordDb;
use bunny_integrations::{build_git_manager, build_hub, ChatBridgeHub, GitWorkspaceManager, IntegrationsDb};
use parking_lot::Mutex as ParkingMutex;
use bunny_browser::BrowserManager;
use bunny_core::config::BunnyConfig;
use bunny_core::redaction::Redactor;
use bunny_pty::TerminalManager;
use bunny_push::FcmClient;
use bunny_secrets::SecretsVault;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct TerminalContextCacheEntry {
    pub cwd: Option<String>,
    pub git_project: Option<String>,
    pub git_branch: Option<String>,
    pub expires_at: Instant,
}

pub struct AppState {
    pub config: BunnyConfig,
    pub auth: AuthService,
    pub terminals: TerminalManager,
    pub browsers: BrowserManager,
    pub redactor: RwLock<Redactor>,
    pub previews: RwLock<HashMap<Uuid, PreviewState>>,
    pub terminal_sessions: RwLock<HashMap<Uuid, Uuid>>,
    pub browser_sessions: RwLock<HashMap<Uuid, Uuid>>,
    pub cdp_collectors: RwLock<HashMap<Uuid, CdpCollectorHandle>>,
    pub realtime: Arc<RealtimeHub>,
    pub watch_hub: Arc<WatchHub>,
    pub data_dir: String,
    pub secrets: Mutex<SecretsVault>,
    /// Passphrase kept in memory while vault is unlocked via API (cleared on lock).
    pub secrets_passphrase: Mutex<Option<String>>,
    pub fcm: FcmClient,
    pub webrtc_sidecar: RwLock<Option<crate::webrtc::WebRtcSidecar>>,
    pub claude_install: Mutex<InstallState>,
    pub claude_auth: Mutex<AuthFlow>,
    pub discord: ParkingMutex<DiscordDb>,
    /// Managed Discord bridge sidecar (spawned by bunny run or restarted via API).
    pub discord_bridge: tokio::sync::Mutex<Option<crate::discord_bridge::DiscordBridgeSidecar>>,
    /// Active headless Claude subprocess PIDs per Discord thread (for /thread/stop).
    pub thread_claude_pids: Mutex<HashMap<String, u32>>,
    pub integrations: ParkingMutex<IntegrationsDb>,
    pub git_workspace: GitWorkspaceManager,
    pub chat_hub: ChatBridgeHub,
    pub git_identity: crate::git_identity::GitIdentityService,
    pub(crate) terminal_context_cache: ParkingMutex<HashMap<Uuid, TerminalContextCacheEntry>>,
}

pub struct PreviewState {
    pub id: Uuid,
    pub session_id: Uuid,
    pub local_port: u16,
    pub public_path: String,
}

impl AppState {
    pub fn new(config: BunnyConfig) -> Result<Self> {
        let data_dir = config.expand_data_dir();
        std::fs::create_dir_all(&data_dir)?;
        let db_path = format!("{data_dir}/bunny.db");
        let discord = DiscordDb::open(&db_path)?;
        let integrations = IntegrationsDb::open(&db_path)?;
        let _ = integrations.migrate_from_discord();
        let chat_hub = build_hub(Arc::new(db_path.clone()));
        let git_workspace = build_git_manager(&data_dir, db_path.clone());
        let git_identity = crate::git_identity::GitIdentityService::new(&data_dir)?;
        let auth = AuthService::new(
            &db_path,
            &data_dir,
            config.security.session_ttl_minutes as i64,
        )?;
        let _ = git_identity.backfill_profile_caches(&auth);
        let secrets_path = std::path::Path::new(&data_dir).join("secrets.enc");
        let secrets = SecretsVault::new(secrets_path);
        let fcm_key = config.push.fcm_server_key.clone().or_else(|| {
            std::env::var("BUNNY_FCM_SERVER_KEY").ok()
        });
        let fcm = FcmClient::new(fcm_key);
        if let Ok(pass) = std::env::var("BUNNY_SECRETS_PASSPHRASE") {
            let mut vault = secrets;
            let mut stored_pass = None;
            if vault.path().exists() {
                if vault.unlock(&pass).is_ok() {
                    stored_pass = Some(pass);
                }
            }
            return Self::from_parts(
                config,
                data_dir,
                auth,
                discord,
                integrations,
                chat_hub,
                git_workspace,
                git_identity,
                vault,
                fcm,
                stored_pass,
            );
        }

        Self::from_parts(
            config,
            data_dir,
            auth,
            discord,
            integrations,
            chat_hub,
            git_workspace,
            git_identity,
            secrets,
            fcm,
            None,
        )
    }

    fn from_parts(
        config: BunnyConfig,
        data_dir: String,
        auth: AuthService,
        discord: DiscordDb,
        integrations: IntegrationsDb,
        chat_hub: ChatBridgeHub,
        git_workspace: GitWorkspaceManager,
        git_identity: crate::git_identity::GitIdentityService,
        secrets: SecretsVault,
        fcm: FcmClient,
        secrets_passphrase: Option<String>,
    ) -> Result<Self> {
        let mut redactor = Redactor::new();
        if secrets.is_unlocked() {
            if let Ok(values) = secrets.all_values() {
                redactor = redactor.with_known_secrets(values);
            }
        }

        Ok(Self {
            auth,
            terminals: TerminalManager::new(
                config.terminal.shell.clone(),
                config.terminal.output_buffer_lines,
                config.terminal.use_tmux(),
                Some(std::path::PathBuf::from(&data_dir).join("terminal-scrollback")),
                std::path::PathBuf::from(&data_dir),
            ),
            browsers: BrowserManager::new(config.browser.width, config.browser.height),
            redactor: RwLock::new(redactor),
            secrets: Mutex::new(secrets),
            secrets_passphrase: Mutex::new(secrets_passphrase),
            fcm,
            webrtc_sidecar: RwLock::new(None),
            claude_install: Mutex::new(InstallState::default()),
            claude_auth: Mutex::new(AuthFlow::default()),
            discord: ParkingMutex::new(discord),
            discord_bridge: tokio::sync::Mutex::new(None),
            thread_claude_pids: Mutex::new(HashMap::new()),
            previews: RwLock::new(HashMap::new()),
            terminal_sessions: RwLock::new(HashMap::new()),
            browser_sessions: RwLock::new(HashMap::new()),
            cdp_collectors: RwLock::new(HashMap::new()),
            realtime: Arc::new(RealtimeHub::new()),
            watch_hub: Arc::new(WatchHub::new()),
            integrations: ParkingMutex::new(integrations),
            git_workspace,
            chat_hub,
            git_identity,
            terminal_context_cache: ParkingMutex::new(HashMap::new()),
            data_dir: data_dir.clone(),
            config,
        })
    }

    pub fn secrets_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.data_dir).join("secrets.enc")
    }

    pub fn refresh_redactor_secrets(&self) {
        let vault = self.secrets.lock();
        let mut redactor = Redactor::new();
        if vault.is_unlocked() {
            if let Ok(values) = vault.all_values() {
                redactor = redactor.with_known_secrets(values);
            }
        }
        *self.redactor.write() = redactor;
    }

    pub fn secret_env_for_session(&self, session_id: Uuid) -> std::collections::HashMap<String, String> {
        let vault = self.secrets.lock();
        if !vault.is_unlocked() {
            return std::collections::HashMap::new();
        }
        vault.env_for_session(session_id).unwrap_or_default()
    }

    pub fn record_timeline(
        &self,
        session_id: Uuid,
        source: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<u64> {
        let redacted = self.redactor.read().redact_json_value(&payload);
        let payload_str = serde_json::to_string(&redacted)?;
        let seq = self.auth.db().lock().next_timeline_sequence(session_id)?;
        self.auth.db().lock().insert_timeline_event(
            Uuid::new_v4(),
            session_id,
            source,
            event_type,
            &payload_str,
            seq,
        )?;
        let event = serde_json::json!({
            "type": "timeline.event",
            "eventId": Uuid::new_v4().to_string(),
            "sequence": seq,
            "source": source,
            "eventType": event_type,
            "payload": redacted,
        });
        self.realtime.publish(session_id, &event);
        self.maybe_push_for_event(session_id, event_type, &redacted);
        Ok(seq)
    }

    pub fn maybe_push_for_event(
        &self,
        session_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
    ) {
        if !self.config.push.enabled {
            return;
        }
        let (title, body) = match event_type {
            "browser.console" => {
                let level = payload
                    .get("level")
                    .or_else(|| payload.get("payload").and_then(|p| p.get("level")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("log");
                if level != "error" && level != "warn" {
                    return;
                }
                (
                    "bunny: console".to_string(),
                    payload
                        .get("text")
                        .or_else(|| payload.get("payload").and_then(|p| p.get("text")))
                        .and_then(|v| v.as_str())
                        .unwrap_or(level)
                        .chars()
                        .take(120)
                        .collect::<String>(),
                )
            }
            "session.status.changed" => (
                "bunny: session".to_string(),
                payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("updated")
                    .to_string(),
            ),
            "terminal.status.changed" => return,
            _ => return,
        };

        let user_ids = self
            .auth
            .db()
            .lock()
            .list_session_member_user_ids(session_id)
            .unwrap_or_default();
        if user_ids.is_empty() {
            if let Ok(owner) = self.auth.owner_id() {
                self.spawn_push_to_user(owner, &title, &body, session_id);
            }
            return;
        }
        for uid in user_ids {
            self.spawn_push_to_user(uid, &title, &body, session_id);
        }
    }

    fn spawn_push_to_user(
        &self,
        user_id: Uuid,
        title: &str,
        body: &str,
        session_id: Uuid,
    ) {
        let tokens = self
            .auth
            .db()
            .lock()
            .list_push_tokens_for_user(user_id)
            .unwrap_or_default();
        if tokens.is_empty() {
            return;
        }
        let fcm = self.fcm.clone();
        let title = title.to_string();
        let body = body.to_string();
        let mut data = serde_json::Map::new();
        data.insert(
            "session_id".into(),
            serde_json::Value::String(session_id.to_string()),
        );
        data.insert("click_action".into(), serde_json::Value::String("FLUTTER_NOTIFICATION_CLICK".into()));
        let message = bunny_push::PushMessage {
            title,
            body,
            data: Some(data),
        };
        tokio::spawn(async move {
            for token in tokens {
                let _ = fcm.send_to_token(&token, &message).await;
            }
        });
    }

    pub async fn start_browser_cdp(
        self: &Arc<Self>,
        stream_session_id: Uuid,
        browser_id: Uuid,
    ) -> Result<()> {
        let cdp_port = self
            .browsers
            .get_cdp_port(browser_id)
            .ok_or_else(|| anyhow::anyhow!("browser not found"))?;
        if self.cdp_collectors.read().contains_key(&browser_id) {
            return Ok(());
        }
        let handle =
            crate::cdp_collector::spawn_cdp_collector(self.clone(), stream_session_id, browser_id, cdp_port)
                .await?;
        self.browser_sessions
            .write()
            .insert(browser_id, stream_session_id);
        self.cdp_collectors.write().insert(browser_id, handle);
        Ok(())
    }
}
