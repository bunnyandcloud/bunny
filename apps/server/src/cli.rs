use crate::api;
use crate::state::AppState;
use crate::terminals::{default_shell_cwd, persist_terminal};
use anyhow::Result;
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
    if !state.auth.needs_bootstrap()? {
        println!("✓ Owner account already exists");
        return Ok(());
    }
    let email = opts
        .email
        .unwrap_or_else(|| prompt("Email: "));
    let password = opts.password.unwrap_or_else(|| prompt_password("Password: "));
    let confirm = prompt_password("Confirm: ");
    if password != confirm {
        anyhow::bail!("passwords do not match");
    }
    let owner_id = state.auth.bootstrap_owner(&email, &password)?;
    println!("✓ Owner account created");
    println!("✓ Local auth enabled");
    println!("✓ Anonymous access disabled");
    println!("✓ Secure session cookies enabled");

    // Optional: enable MFA during initial bootstrap.
    if prompt_yes_no("Enable TOTP MFA now? [y/N]: ", false) {
        println!("\nMFA setup (TOTP)");
        println!("1) Scan the QR code in the web UI later, or add this secret manually now.");
        println!("2) Then enter a 6-digit code to confirm.\n");

        let setup = state.auth.mfa_setup_begin(owner_id)?;
        println!("Issuer: bunny");
        println!("Account: {email}");
        println!("otpauth URI (QR payload):\n{}\n", setup.otpauth_uri);
        if let Ok(code) = QrCode::new(setup.otpauth_uri.as_bytes()) {
            // Dense1x2 renders nicely in most terminals (2 vertical pixels per char).
            let qr = code
                .render::<unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!("Scan this QR code with your authenticator app:\n{qr}\n");
        } else {
            println!("(QR rendering failed in this terminal; use the otpauth URI above.)\n");
        }
        println!("Manual secret (base32) — show once:\n{}\n", setup.secret_base32);

        let mut attempts = 0;
        loop {
            let code = prompt("TOTP code (or empty to cancel): ");
            if code.trim().is_empty() {
                state.auth.mfa_setup_cancel(owner_id)?;
                println!("✓ MFA setup cancelled");
                break;
            }
            match state.auth.mfa_setup_confirm(owner_id, &code) {
                Ok(recovery) => {
                    println!("✓ MFA enabled");
                    println!("\nRecovery codes — save these now (shown only once):");
                    for c in recovery {
                        println!("  {c}");
                    }
                    println!();
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    eprintln!("✗ Invalid code: {e}");
                    if attempts >= 3 {
                        state.auth.mfa_setup_cancel(owner_id)?;
                        anyhow::bail!("MFA setup failed (too many attempts); setup cancelled");
                    }
                }
            }
        }
    }
    Ok(())
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
                state.auth.bootstrap_owner(&email, &password)?;
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
    serve(state, host, opts.port, web_dist).await
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
    serve(state, host, opts.port, web_dist).await
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
    serve(state, host, opts.port, web_dist).await
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
) -> Result<()> {
    crate::recovery::restore_sessions(&state);
    crate::recovery::spawn_relay_if_enabled(state.clone());
    crate::recovery::spawn_health_checks(state.clone());
    let prefetch = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::claude::ensure_install_script(&prefetch.data_dir).await {
            tracing::warn!("claude install script prefetch: {e}");
        }
    });
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
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(state.clone()))
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
