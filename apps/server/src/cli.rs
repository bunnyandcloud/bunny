use crate::api;
use crate::state::AppState;
use crate::terminals::{default_shell_cwd, persist_terminal};
use anyhow::{bail, Result};
use bunny_i18n::{Locale, t};
use dialoguer::Select;
use axum::Router;
use clap::{Parser, Subcommand};
use qrcode::render::unicode;
use qrcode::QrCode;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "bunny", about = "Remote dev/debug agent for Linux servers")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create owner account and local auth (first run)
    Configure(ConfigureOpts),
    /// Initialize auth database
    InitAuth,
    /// Show auth status
    AuthStatus,
    /// User management
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
    /// Start API server
    Start(StartOpts),
    /// Run the agent (builds and serves the web UI by default; use --no-web-ui to skip)
    Run(RunOpts),
    /// Start dev session with optional preview and browser
    Dev(DevOpts),
    /// Stop a session
    Stop {
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Check system dependencies
    Doctor,
    /// Show running status
    Status,
    /// Recover a session
    Recover {
        session_id: String,
    },
    /// Reset a session
    Reset {
        session_id: String,
    },
    /// Systemd service management
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
    /// Encrypted secrets vault (~/.config/bunny/secrets.enc)
    Secrets(crate::secrets_cli::SecretsOpts),
    /// Write default config.yaml if missing (Docker-friendly)
    #[command(name = "config-init")]
    ConfigInit,
    /// Discord bridge helpers
    Discord {
        #[command(subcommand)]
        command: DiscordCommands,
    },
}

#[derive(clap::Subcommand)]
pub enum DiscordCommands {
    /// Generate tokens and write agent + bridge config, then optionally OAuth for user linking
    Setup {
        /// Path for bridge YAML (repo volume: .discord/bridge.yaml on the host)
        #[arg(long, default_value = ".discord/bridge.yaml")]
        bridge_out: String,
        /// Discord application id (or DISCORD_APPLICATION_ID env)
        #[arg(long, env = "DISCORD_APPLICATION_ID")]
        application_id: Option<u64>,
        /// Bot token (or DISCORD_BOT_TOKEN env)
        #[arg(long, env = "DISCORD_BOT_TOKEN")]
        bot_token: Option<String>,
        /// Discord server ID for guild-scoped slash commands (optional; `--guild-id` or edit bridge.yaml later)
        #[arg(long, env = "DISCORD_GUILD_ID")]
        guild_id: Option<u64>,
        /// Skip OAuth user-linking setup (bot/bridge only)
        #[arg(long)]
        skip_oauth: bool,
        /// Configure OAuth only (skip bot/bridge setup)
        #[arg(long)]
        oauth_only: bool,
        /// OAuth client id (defaults to application id)
        #[arg(long, env = "DISCORD_OAUTH_CLIENT_ID")]
        oauth_client_id: Option<String>,
        /// OAuth client secret
        #[arg(long, env = "DISCORD_OAUTH_CLIENT_SECRET")]
        oauth_client_secret: Option<String>,
        /// OAuth redirect URI (default: {public_url}/api/v1/auth/discord/callback)
        #[arg(long, env = "DISCORD_OAUTH_REDIRECT_URI")]
        oauth_redirect_uri: Option<String>,
        /// Public base URL (auto for local dev; prompted or BUNNY_PUBLIC_URL in production)
        #[arg(long, env = "BUNNY_PUBLIC_URL")]
        public_url: Option<String>,
    },
    /// Run the Discord bot (requires a running agent: bunny run)
    Bridge {
        /// Bridge YAML (default: .discord/bridge.yaml or BUNNY_DISCORD_BRIDGE_CONFIG)
        #[arg(long, env = "BUNNY_DISCORD_BRIDGE_CONFIG")]
        config: Option<std::path::PathBuf>,
    },
    /// Sync agent config.yaml from an existing bridge YAML (fixes token mismatch)
    Sync {
        /// Bridge YAML (default: .discord/bridge.yaml)
        #[arg(long, default_value = ".discord/bridge.yaml")]
        bridge_config: String,
    },
    /// [Deprecated] Use `bunny discord setup` — configures OAuth only
    #[command(name = "oauth-setup", hide = true)]
    OauthSetup {
        /// OAuth client id (Discord Application ID)
        #[arg(long, env = "DISCORD_OAUTH_CLIENT_ID")]
        client_id: Option<String>,
        /// OAuth client secret
        #[arg(long, env = "DISCORD_OAUTH_CLIENT_SECRET")]
        client_secret: Option<String>,
        /// OAuth redirect URI (default: {public_url}/api/v1/auth/discord/callback)
        #[arg(long, env = "DISCORD_OAUTH_REDIRECT_URI")]
        redirect_uri: Option<String>,
    },
}

#[derive(Parser)]
pub struct ConfigureOpts {
    pub email: Option<String>,
    #[arg(long)]
    pub password: Option<String>,
}

#[derive(Parser)]
pub struct StartOpts {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value = "7681")]
    pub port: u16,
    /// Do not start the Discord bridge alongside the agent
    #[arg(long = "no-discord-bridge")]
    pub no_discord_bridge: bool,
}

#[derive(Parser)]
pub struct RunOpts {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value = "7681")]
    pub port: u16,
    /// Agent only — do not build or serve apps/web
    #[arg(long = "no-web-ui")]
    pub no_web_ui: bool,
    /// Force a fresh `npm run build` in apps/web before starting
    #[arg(long)]
    pub web_ui_rebuild: bool,
    /// Do not start the Discord bridge alongside the agent
    #[arg(long = "no-discord-bridge")]
    pub no_discord_bridge: bool,
}

#[derive(Parser)]
pub struct DevOpts {
    #[arg(long)]
    pub cmd: Option<String>,
    #[arg(long)]
    pub preview: Option<u16>,
    #[arg(long)]
    pub browser: bool,
    #[arg(long, default_value = "server")]
    pub name: String,
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value = "7681")]
    pub port: u16,
}

#[derive(Subcommand)]
pub enum UserCommands {
    Create { email: String },
    Invite {
        session_id: String,
        email: String,
        #[arg(long, default_value = "viewer")]
        role: String,
    },
    Revoke { email: String },
}

#[derive(Subcommand)]
pub enum ServiceCommands {
    Install,
    Start,
    Stop,
    Status,
    Logs,
}

pub async fn run_configure(state: &AppState, opts: ConfigureOpts) -> Result<()> {
    let locale = prompt_configure_locale();
    if let Some(path) = crate::config_init::ensure_user_config()? {
        println!(
            "{}",
            t(locale, "configure.config_created", &[("path", &path.display().to_string())])
        );
    }
    if !state.auth.needs_bootstrap()? {
        println!("{}", t(locale, "configure.owner_exists", &[]));
        maybe_configure_discord_interactive(state, locale).await?;
        return Ok(());
    }
    let email = opts
        .email
        .unwrap_or_else(|| prompt(&t(locale, "configure.prompt.email", &[])));
    let password = opts
        .password
        .unwrap_or_else(|| prompt_password(&t(locale, "configure.prompt.password", &[])));
    let confirm = prompt_password(&t(locale, "configure.prompt.confirm", &[]));
    if password != confirm {
        anyhow::bail!("{}", t(locale, "configure.password_mismatch", &[]));
    }
    let owner_id = state
        .auth
        .bootstrap_owner(&email, &password, locale.as_str())?;
    println!("{}", t(locale, "configure.owner_created", &[]));
    println!("{}", t(locale, "configure.local_auth_enabled", &[]));
    println!("{}", t(locale, "configure.anonymous_disabled", &[]));
    println!("{}", t(locale, "configure.secure_cookies", &[]));

    if prompt_yes_no(&t(locale, "configure.mfa.enable_prompt", &[]), false) {
        println!("\n{}", t(locale, "configure.mfa.title", &[]));
        println!("{}", t(locale, "configure.mfa.step1", &[]));
        println!("{}\n", t(locale, "configure.mfa.step2", &[]));

        let setup = state.auth.mfa_setup_begin(owner_id)?;
        println!("{}", t(locale, "configure.mfa.issuer", &[]));
        println!(
            "{}",
            t(locale, "configure.mfa.account", &[("email", &email)])
        );
        println!(
            "{}",
            t(
                locale,
                "configure.mfa.otpauth_uri",
                &[("uri", &setup.otpauth_uri)]
            )
        );
        if let Ok(code) = QrCode::new(setup.otpauth_uri.as_bytes()) {
            let qr = code
                .render::<unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!(
                "{}",
                t(locale, "configure.mfa.scan_qr", &[("qr", &qr)])
            );
        } else {
            println!("{}", t(locale, "configure.mfa.qr_failed", &[]));
        }
        println!(
            "{}",
            t(
                locale,
                "configure.mfa.manual_secret",
                &[("secret", &setup.secret_base32)]
            )
        );

        let mut attempts = 0;
        loop {
            let code = prompt(&t(locale, "configure.mfa.prompt_code", &[]));
            if code.trim().is_empty() {
                state.auth.mfa_setup_cancel(owner_id)?;
                println!("{}", t(locale, "configure.mfa.cancelled", &[]));
                break;
            }
            match state.auth.mfa_setup_confirm(owner_id, &code) {
                Ok(recovery) => {
                    println!("{}", t(locale, "configure.mfa.enabled", &[]));
                    println!("\n{}", t(locale, "configure.mfa.recovery_header", &[]));
                    for c in recovery {
                        println!("  {c}");
                    }
                    println!();
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    eprintln!(
                        "{}",
                        t(
                            locale,
                            "configure.mfa.invalid_code",
                            &[("error", &e.to_string())]
                        )
                    );
                    if attempts >= 3 {
                        state.auth.mfa_setup_cancel(owner_id)?;
                        anyhow::bail!("{}", t(locale, "configure.mfa.failed_cancelled", &[]));
                    }
                }
            }
        }
    }

    maybe_configure_discord_interactive(state, locale).await?;
    Ok(())
}

fn prompt_configure_locale() -> Locale {
    let can_prompt = stdin_is_tty() || std::env::var("BUNNY_DOCKER_DEV").ok().as_deref() == Some("1");
    if !can_prompt {
        return Locale::En;
    }
    let options = [
        t(Locale::En, "configure.prompt.language.option_en", &[]),
        t(Locale::En, "configure.prompt.language.option_fr", &[]),
    ];
    let default = if std::env::var("LANG")
        .unwrap_or_default()
        .to_lowercase()
        .starts_with("fr")
    {
        1
    } else {
        0
    };
    let selection = Select::new()
        .with_prompt(&t(Locale::En, "configure.prompt.language", &[]))
        .items(&options)
        .default(default)
        .interact()
        .unwrap_or(default);
    match selection {
        0 => Locale::En,
        _ => Locale::Fr,
    }
}

async fn maybe_configure_discord_interactive(state: &AppState, locale: Locale) -> Result<()> {
    let bridge = std::env::current_dir()?.join(".discord/bridge.yaml");
    let can_prompt = stdin_is_tty() || std::env::var("BUNNY_DOCKER_DEV").ok().as_deref() == Some("1");
    let oauth_ok = crate::discord_ops::discord_oauth_configured(state);

    if bridge.is_file() {
        if crate::config_init::sync_agent_from_bridge_file(&bridge)? {
            println!(
                "\n{}",
                t(
                    locale,
                    "configure.discord.synced",
                    &[("path", &bridge.display().to_string())]
                )
            );
            println!("{}", t(locale, "configure.discord.restart_hint", &[]));
        }
        if oauth_ok {
            println!(
                "\n{}",
                t(
                    locale,
                    "configure.discord.already_configured",
                    &[("path", &bridge.display().to_string())]
                )
            );
            print_discord_run_hints(locale);
            if can_prompt
                && prompt_yes_no(&t(locale, "configure.discord.reconfigure_prompt", &[]), false)
            {
                // fall through to full setup
            } else {
                return Ok(());
            }
        } else {
            println!(
                "\n{}",
                t(
                    locale,
                    "configure.discord.bridge_without_oauth",
                    &[("path", &bridge.display().to_string())]
                )
            );
            println!("{}", t(locale, "configure.discord.portal_link", &[]));
            if can_prompt
                && prompt_yes_no(&t(locale, "configure.discord.oauth_setup_prompt", &[]), true)
            {
                let app_hint = crate::config_init::read_bridge_application_id(&bridge)
                    .map(|id| id.to_string());
                let public_url =
                    resolve_discord_public_url_for_setup(state, locale, can_prompt, None)?;
                run_discord_oauth_setup(
                    state,
                    locale,
                    DiscordOAuthSetupOpts {
                        client_id: None,
                        client_secret: None,
                        redirect_uri: None,
                        application_id_hint: app_hint,
                        public_url: Some(public_url),
                        interactive: true,
                    },
                )
                .await?;
                print_discord_run_hints(locale);
                return Ok(());
            }
            print_discord_run_hints(locale);
            println!("{}", t(locale, "configure.discord.oauth_skipped", &[]));
            return Ok(());
        }
    } else if !can_prompt {
        println!("\n{}", t(locale, "configure.discord.not_configured", &[]));
        print_discord_run_hints(locale);
        println!("{}", t(locale, "configure.discord.or_setup", &[]));
        return Ok(());
    } else if !prompt_yes_no(&t(locale, "configure.discord.setup_prompt", &[]), false) {
        println!("\n{}", t(locale, "configure.discord.skipped", &[]));
        print_discord_run_hints(locale);
        return Ok(());
    }

    let public_url = resolve_discord_public_url_for_setup(state, locale, can_prompt, None)?;

    println!("\n{}", t(locale, "configure.discord.section_bot", &[]));
    println!("{}", t(locale, "configure.discord.portal_link", &[]));

    let app_id: u64 = loop {
        print_discord_hint(locale, "configure.discord.hint_app_id");
        let s = prompt(&t(locale, "configure.discord.prompt_app_id", &[]));
        match s.trim().parse::<u64>() {
            Ok(v) => break v,
            Err(_) => eprintln!("{}", t(locale, "configure.discord.invalid_number", &[])),
        }
    };
    print_discord_hint(locale, "configure.discord.hint_bot_token");
    print_discord_hint(locale, "configure.discord.hint_bot_intents");
    let token = prompt_password(&t(locale, "configure.discord.prompt_token", &[]));
    run_discord_with_locale(
        state,
        locale,
        DiscordCommands::Setup {
            bridge_out: ".discord/bridge.yaml".into(),
            application_id: Some(app_id),
            bot_token: Some(token),
            guild_id: None,
            skip_oauth: false,
            oauth_only: false,
            oauth_client_id: None,
            oauth_client_secret: None,
            oauth_redirect_uri: None,
            public_url: Some(public_url),
        },
    )
    .await
}

fn stdin_is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

pub async fn run_init_auth(state: &AppState) -> Result<()> {
    if state.auth.needs_bootstrap()? {
        println!("No users yet. Run: bunny configure");
    } else {
        println!("✓ Auth database ready at {}/bunny.db", state.data_dir);
    }
    Ok(())
}

pub async fn run_auth_status(state: &AppState) -> Result<()> {
    let needs = state.auth.needs_bootstrap()?;
    println!("require_auth: {}", state.config.security.require_auth);
    println!("needs_bootstrap: {needs}");
    println!("data_dir: {}", state.data_dir);
    Ok(())
}

pub async fn run_user(state: &AppState, command: UserCommands) -> Result<()> {
    match command {
        UserCommands::Create { email } => {
            let password = prompt_password("Password: ");
            if state.auth.needs_bootstrap()? {
                state.auth.bootstrap_owner(&email, &password, "en")?;
                println!("✓ Owner created: {email}");
            } else {
                println!("Use invite flow for additional users (MVP: owner only via configure)");
            }
        }
        UserCommands::Invite {
            session_id,
            email,
            role,
        } => {
            let sid = Uuid::parse_str(&session_id)?;
            let role = bunny_core::permissions::parse_role(&role)
                .ok_or_else(|| anyhow::anyhow!("invalid role"))?;
            // MVP: use nil user as system — production requires authenticated CLI token
            let owner_id = state.auth.owner_id()?;
            let token = state.auth.invite_user(sid, &email, role, owner_id)?;
            println!("✓ Invitation created (token for recipient): {token}");
        }
        UserCommands::Revoke { email } => {
            state.auth.revoke_user_by_email(&email)?;
            println!("✓ Revoked access for {email}");
        }
    }
    Ok(())
}

pub async fn run_start(state: Arc<AppState>, opts: StartOpts) -> Result<()> {
    if state.auth.needs_bootstrap()? {
        anyhow::bail!("No owner account. Run: bunny configure");
    }
    print_banner();
    let host = effective_listen_host(&opts.host, opts.port);
    let web_dist = crate::web_ui::web_dist_dir(crate::web_ui::find_repo_root().as_deref());
    serve(state, host, opts.port, web_dist, !opts.no_discord_bridge).await
}

pub async fn run_run(state: Arc<AppState>, opts: RunOpts) -> Result<()> {
    if state.auth.needs_bootstrap()? {
        anyhow::bail!("No owner account. Run: bunny configure");
    }
    print_banner();

    let serve_web_ui = !opts.no_web_ui;
    let web_dist = if serve_web_ui {
        let repo_root = crate::web_ui::find_repo_root().ok_or_else(|| {
            anyhow::anyhow!(
                "could not find repo root (apps/web/package.json). \
                 Run from the bunny clone or set cwd to the repository root."
            )
        })?;
        std::env::set_current_dir(&repo_root)?;
        if opts.web_ui_rebuild {
            let web_dir = repo_root.join("apps/web");
            let _ = std::fs::remove_dir_all(web_dir.join("dist"));
        }
        Some(crate::web_ui::ensure_web_ui_built(&repo_root)?)
    } else {
        crate::web_ui::web_dist_dir(crate::web_ui::find_repo_root().as_deref())
    };

    let host = effective_listen_host(&opts.host, opts.port);
    let base = format!("http://{}:{}", opts.host, opts.port);
    if web_dist.is_some() {
        println!("✓ Web UI: {base}/");
        println!("✓ Login:  {base}/login");
    }
    serve(state, host, opts.port, web_dist, !opts.no_discord_bridge).await
}

pub async fn run_dev(state: Arc<AppState>, opts: DevOpts) -> Result<()> {
    if state.auth.needs_bootstrap()? {
        anyhow::bail!("No owner account. Run: bunny configure");
    }

    let cwd = default_shell_cwd();
    let owner_id = state.auth.owner_id()?;
    let session_id = state.auth.create_stream_session(owner_id, cwd.to_str().unwrap(), None)?;

    if let Some(cmd) = &opts.cmd {
        let secret_env = state.secret_env_for_session(session_id);
        let (term_id, tmux_target) = state.terminals.create(
            session_id,
            &opts.name,
            &cwd,
            Some(cmd.as_str()),
            80,
            24,
            secret_env,
        )?;
        state.terminal_sessions.write().insert(term_id, session_id);
        let _ = persist_terminal(
            &state,
            term_id,
            session_id,
            &opts.name,
            &state.config.terminal.shell,
            Some(cmd.as_str()),
            &cwd,
            80,
            24,
            tmux_target.as_deref(),
        );
        println!("✓ Terminal started: {}", opts.name);
        println!("✓ Command running: {cmd}");
    }

    if let Some(port) = opts.preview {
        let preview_id = Uuid::new_v4();
        let public_path = format!("/s/{session_id}/ports/{port}/");
        state.previews.write().insert(
            preview_id,
            crate::state::PreviewState {
                id: preview_id,
                session_id,
                local_port: port,
                public_path: public_path.clone(),
            },
        );
        println!("✓ Preview detected: http://127.0.0.1:{port}");
        println!("✓ Preview path: {public_path}");
    }

    if opts.browser {
        let url = opts
            .preview
            .map(|p| format!("http://127.0.0.1:{p}"))
            .unwrap_or_else(|| "http://127.0.0.1:3000".into());
        match state.browsers.create(session_id, &url) {
            Ok(browser_id) => {
                println!("✓ Browser started: Chromium");
                match state.clone().start_browser_cdp(session_id, browser_id).await {
                    Ok(()) => println!("✓ Console/network capture enabled (CDP)"),
                    Err(e) => println!("⚠ CDP collector failed: {e}"),
                }
            }
            Err(e) => println!("⚠ Browser stack failed: {e}"),
        }
    }

    let base = format!("http://{}:{}", opts.host, opts.port);
    println!("✓ Session created: {session_id}");
    println!("✓ Authentication required");
    println!("✓ Anonymous access disabled");
    println!("✓ Secure login URL: {base}/login?next=/s/{session_id}");

    let host = effective_listen_host(&opts.host, opts.port);
    let web_dist = crate::web_ui::web_dist_dir(crate::web_ui::find_repo_root().as_deref());
    serve(state, host, opts.port, web_dist, true).await
}

pub async fn run_stop(_state: &AppState, _session_id: Option<String>) -> Result<()> {
    println!("✓ Session stop requested (restart server to fully stop)");
    Ok(())
}

pub async fn run_doctor() -> Result<()> {
    println!("bunny doctor — checking capabilities\n");
    check_cmd("PTY", native_pty_check);
    check_cmd("WebSocket", || Ok(()));
    if let Some(path) = bunny_browser::resolve_chromium_binary() {
        println!("  ✓ Chromium ({})", path.display());
    } else {
        println!("  ✗ Chromium missing — run ./scripts/install-prerequisites.sh (Playwright in Docker)");
        check_optional("Chromium (legacy)", "chromium-browser", &["--version"]);
        check_optional("Google Chrome", "google-chrome", &["--version"]);
    }
    let xvfb_ok = command_works("Xvfb", &["-help"]);
    if xvfb_ok {
        println!("  ✓ Xvfb");
    } else {
        println!("  ✗ Xvfb missing — Browser tab needs it (./scripts/install-prerequisites.sh)");
    }
    check_optional("x11vnc", "x11vnc", &["-help"]);
    check_optional("websockify", "websockify", &["--help"]);
    if std::path::Path::new("/usr/share/novnc/vnc.html").is_file()
        || std::path::Path::new("/usr/local/share/novnc/vnc.html").is_file()
    {
        println!("  ✓ noVNC static UI (vnc.html)");
    } else {
        println!("  ⚠ noVNC UI missing — apt install novnc (Browser tab needs vnc.html)");
    }
    check_optional("Node.js", "node", &["--version"]);
    check_optional("npm", "npm", &["--version"]);
    if crate::web_ui::find_repo_root().is_some() {
        if crate::web_ui::web_dist_dir(crate::web_ui::find_repo_root().as_deref()).is_some() {
            println!("  ✓ Web UI build (apps/web/dist)");
        } else {
            println!("  ⚠ Web UI not built — `bunny run` will run npm build");
        }
    }
    if bunny_pty::tmux::available() {
        println!("  ✓ tmux (terminals survive agent restarts)");
    } else {
        println!("  ⚠ tmux not found — install tmux for persistent shells");
    }
    if command_works("git", &["--version"]) {
        println!("  ✓ git (Discord thread branches, `/bunny git`)");
    } else {
        println!(
            "  ⚠ git not found — install git for Discord git commands (./scripts/install-prerequisites.sh on Debian/Ubuntu)"
        );
    }
    if crate::webrtc::sidecar_script_path().is_some() {
        println!("  ✓ webrtc-sidecar script");
    } else {
        println!("  ⚠ webrtc-sidecar/index.js not found");
    }
    if crate::cdp_collector::sidecar_script_path().is_some() {
        println!("  ✓ cdp-sidecar script");
    } else {
        println!("  ⚠ cdp-sidecar/index.js not found");
    }
    let (label, ok, hint) = crate::claude::doctor_check();
    if ok {
        println!("  ✓ {label}");
    } else {
        println!("  ⚠ {label} — {hint}");
    }
    println!("\n✓ Doctor complete");
    Ok(())
}

pub async fn run_status(state: &AppState) -> Result<()> {
    println!("terminals: {}", state.terminals.list_ids().len());
    println!("previews: {}", state.previews.read().len());
    println!("bind: {}:{}", state.config.server.bind_host, state.config.server.port);
    Ok(())
}

pub async fn run_recover(_state: &AppState, session_id: String) -> Result<()> {
    println!("✓ Recover session {session_id} — attach to existing PTY if alive");
    Ok(())
}

pub async fn run_reset(state: &AppState, session_id: String) -> Result<()> {
    let id = Uuid::parse_str(&session_id)?;
    for term in state.terminals.list_ids() {
        state.terminals.remove(term);
    }
    state.browsers.stop(id);
    println!("✓ Session {session_id} reset");
    Ok(())
}

pub async fn run_config_init(_state: &AppState) -> Result<()> {
    match crate::config_init::ensure_user_config()? {
        Some(path) => println!("✓ Created {}", path.display()),
        None => println!("✓ Config already exists at {}", crate::config_init::config_path().display()),
    }
    Ok(())
}

pub async fn run_discord(state: &AppState, command: DiscordCommands) -> Result<()> {
    run_discord_with_locale(state, Locale::En, command).await
}

struct DiscordOAuthSetupOpts {
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
    application_id_hint: Option<String>,
    public_url: Option<String>,
    interactive: bool,
}

fn discord_public_url(state: &AppState) -> String {
    state
        .config
        .discord
        .public_url
        .clone()
        .unwrap_or_else(|| default_local_public_url(state))
}

fn default_local_public_url(state: &AppState) -> String {
    format!("http://127.0.0.1:{}", state.config.server.port)
}

fn normalize_public_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn is_localhost_public_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("http://127.0.0.1")
        || lower.starts_with("http://localhost")
        || lower.starts_with("https://127.0.0.1")
        || lower.starts_with("https://localhost")
}

fn discord_deployment_forced_local() -> bool {
    std::env::var("BUNNY_DOCKER_DEV").ok().as_deref() == Some("1")
}

fn discord_deployment_forced_production() -> bool {
    std::env::var("BUNNY_PRODUCTION").ok().as_deref() == Some("1")
}

fn discord_deployment_heuristic_local(state: &AppState) -> bool {
    if discord_deployment_forced_local() {
        return true;
    }
    if discord_deployment_forced_production() {
        return false;
    }
    if std::path::Path::new("/.dockerenv").exists() {
        return false;
    }
    state.config.server.bind_host == "127.0.0.1"
}

fn prompt_discord_deployment_is_local(state: &AppState, locale: Locale) -> Result<bool> {
    if discord_deployment_forced_local() {
        return Ok(true);
    }
    if discord_deployment_forced_production() {
        return Ok(false);
    }
    let options = [
        t(
            locale,
            "configure.discord.deployment_option_local",
            &[],
        ),
        t(
            locale,
            "configure.discord.deployment_option_production",
            &[],
        ),
    ];
    let default = if state.config.server.bind_host == "127.0.0.1" {
        0
    } else {
        1
    };
    let selection = Select::new()
        .with_prompt(&t(locale, "configure.discord.deployment_prompt", &[]))
        .items(&options)
        .default(default)
        .interact()
        .unwrap_or(default);
    Ok(selection == 0)
}

fn resolve_discord_public_url_for_setup(
    state: &AppState,
    locale: Locale,
    interactive: bool,
    explicit: Option<String>,
) -> Result<String> {
    if let Some(url) = explicit.filter(|s| !s.trim().is_empty()) {
        return Ok(normalize_public_url(&url));
    }
    if let Ok(env) = std::env::var("BUNNY_PUBLIC_URL") {
        if !env.trim().is_empty() {
            return Ok(normalize_public_url(&env));
        }
    }
    let is_local = if interactive {
        prompt_discord_deployment_is_local(state, locale)?
    } else {
        discord_deployment_heuristic_local(state)
    };
    if !is_local {
        if let Some(url) = state.config.discord.public_url.as_ref().filter(|u| !u.trim().is_empty())
        {
            if !is_localhost_public_url(url) {
                return Ok(normalize_public_url(url));
            }
        }
    }
    if is_local {
        let url = default_local_public_url(state);
        if interactive {
            println!(
                "{}",
                t(locale, "configure.discord.deployment_local", &[("url", &url)])
            );
        }
        return Ok(url);
    }
    if interactive {
        println!(
            "{}",
            t(locale, "configure.discord.deployment_production", &[])
        );
        print_discord_hint(locale, "configure.discord.hint_public_url");
        let entered = prompt(&t(locale, "configure.discord.prompt_public_url", &[]));
        let url = normalize_public_url(&entered);
        if url.is_empty() {
            bail!("public URL is required for production deployment");
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("public URL must start with http:// or https://");
        }
        Ok(url)
    } else {
        bail!(
            "production deployment requires --public-url or BUNNY_PUBLIC_URL (local dev: set BUNNY_DOCKER_DEV=1)"
        )
    }
}

fn discord_oauth_redirect_from_public(public_url: &str) -> String {
    format!(
        "{}/api/v1/auth/discord/callback",
        public_url.trim_end_matches('/')
    )
}

async fn run_discord_oauth_setup(
    state: &AppState,
    locale: Locale,
    opts: DiscordOAuthSetupOpts,
) -> Result<()> {
    let public_base = opts
        .public_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| discord_public_url(state));
    let default_redirect = discord_oauth_redirect_from_public(&public_base);
    let known_app_id = opts.application_id_hint.as_ref().is_some_and(|s| !s.trim().is_empty());
    if opts.interactive {
        println!();
        println!("{}", t(locale, "configure.discord.portal_link", &[]));
        if known_app_id {
            println!(
                "{}",
                t(locale, "configure.discord.oauth_intro_after_bot", &[])
            );
        } else {
            println!("{}", t(locale, "configure.discord.oauth_intro", &[]));
        }
    }
    let client_id = match opts.client_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => id,
        None if known_app_id => {
            let hint = opts.application_id_hint.as_ref().unwrap().trim().to_string();
            if opts.interactive {
                println!(
                    "{}",
                    t(
                        locale,
                        "configure.discord.oauth_using_client_id",
                        &[("id", &hint)]
                    )
                );
            }
            hint
        }
        None if opts.interactive => {
            print_discord_hint(locale, "configure.discord.hint_oauth_client_id");
            let entered = prompt(&t(locale, "configure.discord.prompt_oauth_client_id", &[]));
            if entered.trim().is_empty() {
                bail!("oauth client id is required");
            }
            entered.trim().to_string()
        }
        None => opts
            .application_id_hint
            .ok_or_else(|| anyhow::anyhow!("oauth client id is required"))?,
    };

    let client_secret = match opts.client_secret.filter(|s| !s.trim().is_empty()) {
        Some(secret) => secret,
        None if opts.interactive => {
            if known_app_id {
                print_discord_hint(locale, "configure.discord.oauth_secret_only_hint");
            } else {
                print_discord_hint(locale, "configure.discord.hint_oauth_client_secret");
            }
            prompt_password(&t(locale, "configure.discord.prompt_oauth_client_secret", &[]))
        }
        None => bail!("oauth client secret is required"),
    };

    let redirect = match opts.redirect_uri.filter(|s| !s.trim().is_empty()) {
        Some(uri) => uri,
        None => {
            if opts.interactive {
                println!(
                    "{}",
                    t(
                        locale,
                        "configure.discord.oauth_using_redirect",
                        &[("uri", &default_redirect)]
                    )
                );
            }
            default_redirect
        }
    };

    let path = crate::config_init::apply_oauth_to_config(
        client_id.trim(),
        client_secret.trim(),
        redirect.trim(),
    )?;
    println!(
        "{}",
        t(
            locale,
            "configure.discord.oauth_saved",
            &[("path", &path.display().to_string())]
        )
    );
    println!(
        "\n{}",
        t(locale, "configure.discord.oauth_callback_hint", &[])
    );
    println!("  {}", redirect.trim());
    println!("{}", t(locale, "configure.discord.oauth_callback_steps", &[]));
    println!("{}", t(locale, "configure.discord.restart_hint", &[]));
    Ok(())
}

async fn run_discord_bot_setup(
    _state: &AppState,
    locale: Locale,
    bridge_out: &str,
    application_id: u64,
    bot_token: &str,
    public_url: &str,
    guild_id: Option<u64>,
) -> Result<()> {
    let (plain, hash) = crate::config_init::generate_bridge_credentials();
    let agent_path = crate::config_init::apply_discord_to_config(&hash, public_url)?;
    println!(
        "{}",
        t(
            locale,
            "configure.discord.agent_config",
            &[("path", &agent_path.display().to_string())]
        )
    );
    let bridge_path = std::path::Path::new(bridge_out);
    let bridge_path = if bridge_path.is_absolute() {
        bridge_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(bridge_path)
    };
    crate::config_init::write_bridge_dev_file(
        &bridge_path,
        application_id,
        bot_token,
        &plain,
        "http://127.0.0.1:7681",
        public_url,
        guild_id,
    )?;
    println!(
        "{}",
        t(
            locale,
            "configure.discord.bridge_config",
            &[("path", &bridge_path.display().to_string())]
        )
    );
    Ok(())
}

async fn maybe_run_oauth_after_bot(
    state: &AppState,
    locale: Locale,
    application_id: u64,
    public_url: String,
    skip_oauth: bool,
    oauth_client_id: Option<String>,
    oauth_client_secret: Option<String>,
    oauth_redirect_uri: Option<String>,
) -> Result<()> {
    if skip_oauth {
        return Ok(());
    }
    let has_oauth_flags = oauth_client_id.as_ref().is_some_and(|s| !s.trim().is_empty())
        && oauth_client_secret.as_ref().is_some_and(|s| !s.trim().is_empty());
    if has_oauth_flags {
        return run_discord_oauth_setup(
            state,
            locale,
            DiscordOAuthSetupOpts {
                client_id: oauth_client_id.or_else(|| Some(application_id.to_string())),
                client_secret: oauth_client_secret,
                redirect_uri: oauth_redirect_uri,
                application_id_hint: Some(application_id.to_string()),
                public_url: Some(public_url.clone()),
                interactive: false,
            },
        )
        .await;
    }
    if crate::discord_ops::discord_oauth_configured(state) {
        return Ok(());
    }
    if stdin_is_tty() {
        println!("\n{}", t(locale, "configure.discord.oauth_phase_title", &[]));
        if prompt_yes_no(&t(locale, "configure.discord.oauth_setup_prompt", &[]), true) {
            run_discord_oauth_setup(
                state,
                locale,
                DiscordOAuthSetupOpts {
                    client_id: None,
                    client_secret: None,
                    redirect_uri: oauth_redirect_uri,
                    application_id_hint: Some(application_id.to_string()),
                    public_url: Some(public_url),
                    interactive: true,
                },
            )
            .await?;
        } else {
            println!("{}", t(locale, "configure.discord.oauth_skipped", &[]));
        }
    } else {
        println!("{}", t(locale, "configure.discord.oauth_skipped_noninteractive", &[]));
    }
    Ok(())
}

pub async fn run_discord_with_locale(
    state: &AppState,
    locale: Locale,
    command: DiscordCommands,
) -> Result<()> {
    match command {
        DiscordCommands::Bridge { config } => run_discord_bridge(config),
        DiscordCommands::Sync { bridge_config } => {
            let bridge_path = std::path::Path::new(&bridge_config);
            let bridge_path = if bridge_path.is_absolute() {
                bridge_path.to_path_buf()
            } else {
                std::env::current_dir()?.join(bridge_path)
            };
            if crate::config_init::sync_agent_from_bridge_file(&bridge_path)? {
                println!("✓ Agent config synced from {}", bridge_path.display());
                println!("  Restart bunny run, then retry /bunny link.");
            } else {
                println!("✓ Agent config already matches {}", bridge_path.display());
            }
            Ok(())
        }
        DiscordCommands::Setup {
            bridge_out,
            application_id,
            bot_token,
            guild_id,
            skip_oauth,
            oauth_only,
            oauth_client_id,
            oauth_client_secret,
            oauth_redirect_uri,
            public_url: setup_public_url,
        } => {
            let interactive = stdin_is_tty();
            let public_url = resolve_discord_public_url_for_setup(
                state,
                locale,
                interactive,
                setup_public_url,
            )?;
            if oauth_only {
                let oauth_interactive = oauth_client_secret.is_none() && interactive;
                return run_discord_oauth_setup(
                    state,
                    locale,
                    DiscordOAuthSetupOpts {
                        client_id: oauth_client_id,
                        client_secret: oauth_client_secret,
                        redirect_uri: oauth_redirect_uri,
                        application_id_hint: application_id.map(|id| id.to_string()),
                        public_url: Some(public_url),
                        interactive: oauth_interactive,
                    },
                )
                .await;
            }
            let app_id = application_id
                .ok_or_else(|| anyhow::anyhow!("set --application-id or DISCORD_APPLICATION_ID"))?;
            let token = bot_token
                .ok_or_else(|| anyhow::anyhow!("set --bot-token or DISCORD_BOT_TOKEN"))?;
            run_discord_bot_setup(
                state,
                locale,
                &bridge_out,
                app_id,
                &token,
                &public_url,
                guild_id,
            )
            .await?;
            maybe_run_oauth_after_bot(
                state,
                locale,
                app_id,
                public_url,
                skip_oauth,
                oauth_client_id,
                oauth_client_secret,
                oauth_redirect_uri,
            )
            .await?;
            print_discord_run_hints(locale);
            Ok(())
        }
        DiscordCommands::OauthSetup {
            client_id,
            client_secret,
            redirect_uri,
        } => {
            let public_url =
                resolve_discord_public_url_for_setup(state, locale, stdin_is_tty(), None)?;
            run_discord_oauth_setup(
                state,
                locale,
                DiscordOAuthSetupOpts {
                    client_id,
                    client_secret,
                    redirect_uri,
                    application_id_hint: None,
                    public_url: Some(public_url),
                    interactive: stdin_is_tty(),
                },
            )
            .await
        }
    }
}

fn print_discord_run_hints(locale: Locale) {
    println!("\n{}", t(locale, "configure.discord.run_hints_title", &[]));
    println!("{}", t(locale, "configure.discord.run_terminal1", &[]));
    println!("{}", t(locale, "configure.discord.run_terminal2", &[]));
}

fn print_discord_hint(locale: Locale, key: &str) {
    println!("  {}", t(locale, key, &[]));
}

fn resolve_bridge_config_path(explicit: Option<std::path::PathBuf>) -> Result<std::path::PathBuf> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    if let Ok(env) = std::env::var("BUNNY_DISCORD_BRIDGE_CONFIG") {
        if !env.is_empty() {
            return Ok(std::path::PathBuf::from(env));
        }
    }
    let local = std::env::current_dir()?.join(".discord/bridge.yaml");
    if local.is_file() {
        return Ok(local);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    Ok(std::path::Path::new(&home)
        .join(".config/bunny/discord-bridge.yaml"))
}

fn workspace_root() -> Result<std::path::PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            let text = std::fs::read_to_string(&manifest)?;
            if text.contains("[workspace]") {
                return Ok(dir);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    anyhow::bail!("run from the bunny repo root (workspace Cargo.toml not found)")
}

fn agent_info_reachable() -> bool {
    std::process::Command::new("curl")
        .args(["-sf", "http://127.0.0.1:7681/api/v1/agent/info"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn resolve_bridge_binary(root: &std::path::Path) -> std::path::PathBuf {
    let debug = root.join("target/debug/bunny-discord-bridge");
    let release = root.join("target/release/bunny-discord-bridge");
    if let Ok(path) = std::env::var("BUNNY_DISCORD_BRIDGE_BIN") {
        if !path.is_empty() {
            return std::path::PathBuf::from(path);
        }
    }
    // `cargo build -p bunny-discord-bridge` writes debug; prefer it when newer than release.
    if debug.is_file() {
        if !release.is_file() {
            return debug;
        }
        let debug_mtime = debug.metadata().and_then(|m| m.modified()).ok();
        let release_mtime = release.metadata().and_then(|m| m.modified()).ok();
        if debug_mtime >= release_mtime {
            return debug;
        }
    }
    release
}

fn run_discord_bridge(config: Option<std::path::PathBuf>) -> Result<()> {
    if !agent_info_reachable() {
        eprintln!("⚠ Agent not running — start it first: bunny run");
    }
    let (cfg, bridge_bin) = prepare_discord_bridge(config)?;
    if crate::config_init::sync_agent_from_bridge_file(&cfg)? {
        eprintln!(
            "→ Agent config synced from {} — restart bunny run if it is already running",
            cfg.display()
        );
    }
    eprintln!("→ Discord bridge ({}, {})", cfg.display(), bridge_bin.display());
    let status = std::process::Command::new(&bridge_bin)
        .env("BUNNY_DISCORD_BRIDGE_CONFIG", &cfg)
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "bunny_discord_bridge=info,serenity=warn".into()),
        )
        .status()?;
    if !status.success() {
        bail!("discord bridge exited with {status}");
    }
    Ok(())
}

fn prepare_discord_bridge(explicit_config: Option<std::path::PathBuf>) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let cfg = resolve_bridge_config_path(explicit_config)?;
    if !cfg.is_file() {
        bail!(
            "bridge config not found at {}\n  run: bunny discord setup",
            cfg.display()
        );
    }
    let bridge_bin = ensure_discord_bridge_binary()?;
    Ok((cfg, bridge_bin))
}

fn ensure_discord_bridge_binary() -> Result<std::path::PathBuf> {
    if let Some(bin) = locate_discord_bridge_binary() {
        return Ok(bin);
    }
    build_discord_bridge_binary()?;
    locate_discord_bridge_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "bunny-discord-bridge binary not found after build (set BUNNY_DISCORD_BRIDGE_BIN)"
        )
    })
}

fn build_discord_bridge_binary() -> Result<()> {
    let root = workspace_root()?;
    eprintln!("→ Building discord bridge (first time)…");
    let status = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["build", "--release", "-p", "bunny-discord-bridge", "-q"])
        .status()?;
    if !status.success() {
        bail!("failed to build bunny-discord-bridge");
    }
    Ok(())
}

fn locate_discord_bridge_binary() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("BUNNY_DISCORD_BRIDGE_BIN") {
        if !path.is_empty() {
            let p = std::path::PathBuf::from(path);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Ok(root) = workspace_root() {
        let bin = resolve_bridge_binary(&root);
        if bin.is_file() {
            return Some(bin);
        }
    }
    None
}

pub async fn run_service(command: ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::Install => {
            println!("Install systemd unit from infra/systemd/bunny-agent.service");
        }
        ServiceCommands::Start => println!("sudo systemctl start bunny-agent"),
        ServiceCommands::Stop => println!("sudo systemctl stop bunny-agent"),
        ServiceCommands::Status => println!("sudo systemctl status bunny-agent"),
        ServiceCommands::Logs => println!("journalctl -u bunny-agent -f"),
    }
    Ok(())
}

async fn serve(
    state: Arc<AppState>,
    host: String,
    port: u16,
    web_dist: Option<std::path::PathBuf>,
    auto_discord_bridge: bool,
) -> Result<()> {
    crate::recovery::restore_sessions(&state);
    crate::recovery::spawn_relay_if_enabled(state.clone());
    crate::recovery::spawn_health_checks(state.clone());
    crate::discord_follow::spawn_follow_worker(state.clone());
    let prefetch = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::claude::ensure_install_script(&prefetch.data_dir).await {
            tracing::warn!("claude install script prefetch: {e}");
        }
    });
    crate::discord_bridge::spawn_prefetch_binary();
    if state.config.webrtc.enabled {
        match crate::webrtc::spawn_webrtc_sidecar(state.clone()).await {
            Ok(sidecar) => {
                *state.webrtc_sidecar.write() = Some(sidecar);
                println!("✓ WebRTC sidecar on port {}", state.config.webrtc.sidecar_port);
            }
            Err(e) => println!("⚠ WebRTC sidecar not started: {e}"),
        }
    }

    let app: Router = api::router(state.clone(), web_dist);
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("✓ Listening on http://{addr}");

    if auto_discord_bridge {
        let bridge_state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::discord_bridge::start_managed(&bridge_state).await {
                eprintln!("⚠ Discord bridge not started: {e}");
            }
        });
    }

    let flush_state = state.clone();
    let bridge_shutdown_state = state.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal(flush_state).await;
            crate::discord_bridge::shutdown_managed(&bridge_shutdown_state).await;
        })
        .await?;
    state.terminals.flush_all_scrollbacks();
    Ok(())
}

async fn shutdown_signal(state: Arc<AppState>) {
    let _ = signal::ctrl_c().await;
    state.terminals.flush_all_scrollbacks();
    tracing::info!("terminal scrollback flushed");
}

fn print_banner() {
    println!("bunny agent v{}", env!("CARGO_PKG_VERSION"));
}

/// In Docker, published ports reach the container via non-loopback interfaces — bind 0.0.0.0.
fn effective_listen_host(cli_host: &str, port: u16) -> String {
    if cli_host != "127.0.0.1" {
        return cli_host.to_string();
    }
    if std::path::Path::new("/.dockerenv").exists() {
        println!(
            "✓ Docker detected — listening on 0.0.0.0:{port} (open http://127.0.0.1:{port} on your host)"
        );
        return "0.0.0.0".into();
    }
    cli_host.to_string()
}

fn prompt(label: &str) -> String {
    use std::io::{self, Write};
    print!("{label}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    io::stdin().read_line(&mut s).unwrap();
    s.trim().to_string()
}

fn prompt_password(label: &str) -> String {
    prompt(label)
}

fn prompt_yes_no(label: &str, default: bool) -> bool {
    let s = prompt(label);
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return default;
    }
    matches!(s.as_str(), "y" | "yes" | "o" | "oui")
}

fn check_cmd(name: &str, f: impl FnOnce() -> Result<()>) {
    match f() {
        Ok(()) => println!("  ✓ {name}"),
        Err(e) => println!("  ✗ {name}: {e}"),
    }
}

fn check_optional(name: &str, cmd: &str, args: &[&str]) {
    if command_works(cmd, args) {
        println!("  ✓ {name}");
    } else {
        println!("  ⚠ {name} not found ({cmd})");
    }
}

fn command_works(cmd: &str, args: &[&str]) -> bool {
    match std::process::Command::new(cmd).args(args).output() {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => true,
        _ => false,
    }
}

fn native_pty_check() -> Result<()> {
    // PTY support verified via bunny-pty crate at runtime
    Ok(())
}
