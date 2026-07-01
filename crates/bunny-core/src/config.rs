use figment::{providers::{Env, Format, Serialized, Yaml}, Figment};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BunnyConfig {
    pub auth: AuthConfig,
    pub server: ServerConfig,
    pub security: SecurityConfig,
    pub browser: BrowserConfig,
    pub terminal: TerminalConfig,
    pub network: NetworkConfig,
    pub recovery: RecoveryConfig,
    pub voice: VoiceConfig,
    #[serde(default)]
    pub push: PushConfig,
    #[serde(default)]
    pub webrtc: WebRtcConfig,
    #[serde(default)]
    pub team_chats: TeamChatsConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChatsConfig {
    /// TTL for session ↔ channel link codes (all team chat connectors).
    #[serde(default = "default_link_code_ttl")]
    pub link_code_ttl_minutes: u64,
}

impl Default for TeamChatsConfig {
    fn default() -> Self {
        Self {
            link_code_ttl_minutes: default_link_code_ttl(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    /// Max agent turns per invocation (`--max-turns` for Claude Code and future agents).
    #[serde(default = "default_agent_max_turns")]
    pub max_turns: u32,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            max_turns: default_agent_max_turns(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    /// SHA-256 hex of bridge bearer token (set via `bunny discord token`).
    #[serde(default)]
    pub bridge_token_hash: Option<String>,
    #[serde(default = "default_discord_oauth_client_id")]
    pub oauth_client_id: Option<String>,
    #[serde(default)]
    pub oauth_client_secret: Option<String>,
    #[serde(default)]
    pub oauth_redirect_uri: Option<String>,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bridge_token_hash: None,
            oauth_client_id: default_discord_oauth_client_id(),
            oauth_client_secret: None,
            oauth_redirect_uri: None,
        }
    }
}

fn default_discord_oauth_client_id() -> Option<String> {
    None
}

fn default_link_code_ttl() -> u64 {
    15
}

fn default_agent_max_turns() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_mode")]
    pub mode: String,
    #[serde(default)]
    pub relay_url: Option<String>,
    #[serde(default = "default_true")]
    pub require_auth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_host")]
    pub bind_host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// Base URL browsers use to reach this agent (watch links, OAuth redirects, etc.).
    #[serde(default)]
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_role")]
    pub default_role: String,
    #[serde(default = "default_true")]
    pub require_auth: bool,
    #[serde(default = "default_session_ttl")]
    pub session_ttl_minutes: u64,
    #[serde(default = "default_true")]
    pub redact_secrets: bool,
    #[serde(default = "default_false")]
    pub expose_cdp: bool,
    #[serde(default = "default_false")]
    pub expose_vnc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_browser_width")]
    pub width: u32,
    #[serde(default = "default_browser_height")]
    pub height: u32,
    #[serde(default = "default_true")]
    pub ephemeral_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default = "default_env_mode")]
    pub env_mode: String,
    #[serde(default = "default_buffer_lines")]
    pub output_buffer_lines: usize,
    /// `tmux` keeps shells alive across agent restarts (recommended on Linux).
    #[serde(default = "default_terminal_backend")]
    pub backend: String,
    /// Structured notebook shell UI (blocks + timeline) instead of scrollback-only xterm.
    #[serde(default = "default_true")]
    pub notebook_shells: bool,
}

impl TerminalConfig {
    pub fn use_tmux(&self) -> bool {
        self.backend == "tmux"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_true")]
    pub collect_metadata: bool,
    #[serde(default = "default_false")]
    pub collect_headers: bool,
    #[serde(default = "default_false")]
    pub collect_bodies: bool,
    #[serde(default = "default_true")]
    pub redact_sensitive_headers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_state_store")]
    pub state_store: String,
    pub process_supervisor: ProcessSupervisorConfig,
    pub terminal: ComponentRecoveryConfig,
    pub browser: BrowserRecoveryConfig,
    pub relay: RelayRecoveryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSupervisorConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    #[serde(default = "default_restart_window")]
    pub restart_window_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRecoveryConfig {
    #[serde(default = "default_true")]
    pub preserve_buffer: bool,
    #[serde(default = "default_buffer_lines")]
    pub buffer_lines: usize,
    #[serde(default = "default_manual_policy")]
    pub restart_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserRecoveryConfig {
    #[serde(default = "default_on_failure")]
    pub restart_policy: String,
    #[serde(default = "default_true")]
    pub restore_last_url: bool,
    #[serde(default = "default_true")]
    pub ephemeral_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayRecoveryConfig {
    #[serde(default = "default_always")]
    pub restart_policy: String,
    #[serde(default = "default_backoff_initial")]
    pub backoff_initial_ms: u64,
    #[serde(default = "default_backoff_max")]
    pub backoff_max_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PushConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub fcm_server_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebRtcConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_webrtc_port")]
    pub sidecar_port: u16,
    #[serde(default = "default_stun_urls")]
    pub stun_urls: Vec<String>,
    #[serde(default)]
    pub turn_url: Option<String>,
    #[serde(default)]
    pub turn_username: Option<String>,
    #[serde(default)]
    pub turn_credential: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_voice_provider")]
    pub provider: String,
    #[serde(default = "default_true")]
    pub push_to_talk: bool,
    #[serde(default = "default_false")]
    pub wake_phrase: bool,
    #[serde(default = "default_true")]
    pub require_confirmation: bool,
    #[serde(default = "default_false")]
    pub allow_direct_run: bool,
    #[serde(default = "default_false")]
    pub store_audio: bool,
    #[serde(default = "default_false")]
    pub store_transcript: bool,
    #[serde(default = "default_true")]
    pub redact_transcript: bool,
}

fn default_auth_mode() -> String { "local".into() }
fn default_bind_host() -> String { "127.0.0.1".into() }
fn default_port() -> u16 { 7681 }
fn default_data_dir() -> String { "~/.config/bunny".into() }
fn default_role() -> String { "viewer".into() }
fn default_true() -> bool { true }
fn default_false() -> bool { false }
fn default_session_ttl() -> u64 { 120 }
fn default_shell() -> String { "/bin/bash".into() }
fn default_env_mode() -> String { "allowlist".into() }
fn default_buffer_lines() -> usize { 5000 }
fn default_terminal_backend() -> String { "tmux".into() }
fn default_browser_width() -> u32 { 1440 }
fn default_browser_height() -> u32 { 900 }
fn default_state_store() -> String { "sqlite".into() }
fn default_max_restarts() -> u32 { 5 }
fn default_restart_window() -> u64 { 60 }
fn default_manual_policy() -> String { "manual".into() }
fn default_on_failure() -> String { "on-failure".into() }
fn default_always() -> String { "always".into() }
fn default_backoff_initial() -> u64 { 500 }
fn default_backoff_max() -> u64 { 30000 }
fn default_voice_provider() -> String { "native".into() }
fn default_webrtc_port() -> u16 { 18782 }
fn default_stun_urls() -> Vec<String> {
    vec!["stun:stun.l.google.com:19302".into()]
}

impl Default for BunnyConfig {
    fn default() -> Self {
        Self {
            auth: AuthConfig {
                mode: default_auth_mode(),
                relay_url: None,
                require_auth: true,
            },
            server: ServerConfig {
                bind_host: default_bind_host(),
                port: default_port(),
                data_dir: default_data_dir(),
                public_url: None,
            },
            security: SecurityConfig {
                default_role: default_role(),
                require_auth: true,
                session_ttl_minutes: default_session_ttl(),
                redact_secrets: true,
                expose_cdp: false,
                expose_vnc: false,
            },
            browser: BrowserConfig {
                enabled: true,
                width: default_browser_width(),
                height: default_browser_height(),
                ephemeral_profile: true,
            },
            terminal: TerminalConfig {
                shell: default_shell(),
                env_mode: default_env_mode(),
                output_buffer_lines: default_buffer_lines(),
                backend: default_terminal_backend(),
                notebook_shells: true,
            },
            network: NetworkConfig {
                collect_metadata: true,
                collect_headers: false,
                collect_bodies: false,
                redact_sensitive_headers: true,
            },
            recovery: RecoveryConfig {
                enabled: true,
                state_store: default_state_store(),
                process_supervisor: ProcessSupervisorConfig {
                    enabled: true,
                    max_restarts: default_max_restarts(),
                    restart_window_seconds: default_restart_window(),
                },
                terminal: ComponentRecoveryConfig {
                    preserve_buffer: true,
                    buffer_lines: default_buffer_lines(),
                    restart_policy: default_manual_policy(),
                },
                browser: BrowserRecoveryConfig {
                    restart_policy: default_on_failure(),
                    restore_last_url: true,
                    ephemeral_profile: true,
                },
                relay: RelayRecoveryConfig {
                    restart_policy: default_always(),
                    backoff_initial_ms: default_backoff_initial(),
                    backoff_max_ms: default_backoff_max(),
                },
            },
            voice: VoiceConfig {
                enabled: true,
                provider: default_voice_provider(),
                push_to_talk: true,
                wake_phrase: false,
                require_confirmation: true,
                allow_direct_run: false,
                store_audio: false,
                store_transcript: false,
                redact_transcript: true,
            },
            push: PushConfig {
                enabled: true,
                fcm_server_key: None,
            },
            webrtc: WebRtcConfig {
                enabled: true,
                sidecar_port: default_webrtc_port(),
                stun_urls: default_stun_urls(),
                turn_url: None,
                turn_username: None,
                turn_credential: None,
            },
            team_chats: TeamChatsConfig::default(),
            agents: AgentsConfig::default(),
            discord: DiscordConfig::default(),
        }
    }
}

impl BunnyConfig {
    pub fn load(paths: &[&str]) -> anyhow::Result<Self> {
        let mut figment = Figment::new().merge(Serialized::defaults(BunnyConfig::default()));
        for path in paths {
            if std::path::Path::new(path).exists() {
                figment = figment.merge(Yaml::file(path));
            }
        }
        figment = figment.merge(Env::prefixed("BUNNY_").split("_"));
        Ok(figment.extract()?)
    }

    pub fn expand_data_dir(&self) -> String {
        if self.server.data_dir.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return self.server.data_dir.replacen('~', &home, 1);
            }
        }
        self.server.data_dir.clone()
    }

    pub fn resolve_public_url(&self) -> String {
        self.server
            .public_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| format!("http://127.0.0.1:{}", self.server.port))
    }
}
