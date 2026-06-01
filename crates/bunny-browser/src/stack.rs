use anyhow::Result;
use bunny_core::types::BrowserStatus;
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserStackConfig {
    pub width: u32,
    pub height: u32,
    pub target_url: String,
    pub ephemeral_profile: bool,
    /// Unique Chromium profile directory (avoids collisions between sessions).
    pub profile_dir: Option<String>,
}

pub struct BrowserStack {
    pub x11_display: String,
    pub cdp_port: u16,
    pub vnc_port: u16,
    pub novnc_port: u16,
    pub profile_dir: Option<String>,
    pub status: BrowserStatus,
}

impl BrowserStack {
    pub fn start(config: &BrowserStackConfig) -> Result<Self> {
        let x11_display = pick_display()?;
        let cdp_port = pick_port(9222)?;
        let vnc_port = pick_port(5900)?;
        let novnc_port = pick_port(6080)?;

        info!(%x11_display, cdp_port, vnc_port, novnc_port, "starting browser stack");

        spawn_xvfb(&x11_display, config.width, config.height)?;
        wait_for_xvfb(&x11_display, Duration::from_secs(5))?;

        // noVNC can connect as soon as Xvfb + VNC + websockify are up; Chromium may still be loading.
        spawn_vnc(&x11_display, vnc_port);
        spawn_websockify(vnc_port, novnc_port);
        if !wait_for_tcp_port(novnc_port, Duration::from_secs(8)) {
            warn!(novnc_port, "websockify not accepting connections yet");
        }

        let profile_dir = config.profile_dir.clone();
        spawn_chromium(
            &x11_display,
            cdp_port,
            &config.target_url,
            config.width,
            config.height,
            config.ephemeral_profile,
            profile_dir.as_deref(),
        )?;

        Ok(Self {
            x11_display,
            cdp_port,
            vnc_port,
            novnc_port,
            profile_dir,
            status: BrowserStatus::Running,
        })
    }

    pub fn stop(&mut self) {
        kill_matching(&format!("websockify.*127.0.0.1:{}", self.vnc_port));
        kill_matching(&format!("-rfbport {}", self.vnc_port));
        kill_matching(&format!("Xvfb {}", self.x11_display));
        kill_matching(&format!("DISPLAY={}", self.x11_display));
        if let Some(dir) = &self.profile_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
        self.status = BrowserStatus::Stopped;
    }

    pub fn is_running(&mut self) -> bool {
        self.status == BrowserStatus::Running
    }
}

fn kill_matching(pattern: &str) {
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(pattern)
        .status();
}

fn pick_display() -> Result<String> {
    for n in 99..120u32 {
        let lock = format!("/tmp/.X{n}-lock");
        if !std::path::Path::new(&lock).exists() {
            return Ok(format!(":{n}"));
        }
    }
    Ok(":99".to_string())
}

fn spawn_xvfb(display: &str, width: u32, height: u32) -> Result<()> {
    let screen = format!("{width}x{height}x24");
    Command::new("Xvfb")
        .arg(display)
        .arg("-screen")
        .arg("0")
        .arg(&screen)
        .arg("-ac")
        .arg("+extension")
        .arg("XTEST")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start Xvfb: {e}. Install xvfb package."))?;
    Ok(())
}

fn apply_chromium_args(
    cmd: &mut std::process::Command,
    cdp_port: u16,
    url: &str,
    width: u32,
    height: u32,
    profile_dir: Option<&str>,
) {
    cmd.arg(format!("--remote-debugging-port={cdp_port}"))
        .arg(format!("--window-size={width},{height}"))
        .arg("--window-position=0,0")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-background-timer-throttling")
        .arg("--disable-renderer-backgrounding")
        .arg(url);
    if let Some(dir) = profile_dir {
        cmd.arg(format!("--user-data-dir={dir}"));
    }
}

pub fn resolve_chromium_binary() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("BUNNY_CHROMIUM_PATH") {
        let path = std::path::PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    for candidate in [
        "/usr/local/bin/chromium",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
    ] {
        let path = std::path::PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    find_playwright_chromium()
}

fn find_playwright_chromium() -> Option<std::path::PathBuf> {
    let cache =
        std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".cache/ms-playwright");
    if !cache.is_dir() {
        return None;
    }
    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&cache).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("chromium-") {
            continue;
        }
        let chrome = entry.path().join("chrome-linux/chrome");
        if chrome.is_file() {
            versions.push((name, chrome));
        }
    }
    versions.sort_by(|a, b| a.0.cmp(&b.0));
    versions.pop().map(|(_, path)| path)
}

fn spawn_chromium(
    display: &str,
    cdp_port: u16,
    url: &str,
    width: u32,
    height: u32,
    ephemeral: bool,
    profile_dir: Option<&str>,
) -> Result<()> {
    let chromium = resolve_chromium_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "Chromium not found. Run: ./scripts/install-prerequisites.sh \
             (installs Playwright Chromium in Docker) then: bunny doctor"
        )
    })?;

    let profile = profile_dir.map(|s| s.to_string()).or_else(|| {
        ephemeral.then(|| "/tmp/bunny-chromium-profile".to_string())
    });

    let mut cmd = Command::new(&chromium);
    cmd.env("DISPLAY", display);
    apply_chromium_args(&mut cmd, cdp_port, url, width, height, profile.as_deref());
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start chromium ({}): {e}", chromium.display()))?;
    Ok(())
}

fn spawn_vnc(display: &str, port: u16) -> Option<()> {
    // View-only noVNC (watch / Stream) cannot use a local cursor — the pointer must
    // appear in the framebuffer. XFIXES would override "-cursor X" with a transparent
    // remote cursor that view-only clients never render; disable it explicitly.
    match Command::new("x11vnc")
        .env("DISPLAY", display)
        .arg("-display")
        .arg(display)
        .arg("-rfbport")
        .arg(port.to_string())
        .arg("-localhost")
        .arg("-nopw")
        .arg("-forever")
        .arg("-shared")
        .arg("-noxdamage")
        .arg("-noxfixes")
        .arg("-cursor")
        .arg("X")
        .arg("-cursorpos")
        .arg("-pointer_mode")
        .arg("2")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => Some(()),
        Err(e) => {
            warn!("x11vnc not available: {e}");
            None
        }
    }
}

fn spawn_websockify(vnc_port: u16, listen_port: u16) -> Option<()> {
    let mut cmd = Command::new("websockify");
    cmd.arg(listen_port.to_string())
        .arg(format!("127.0.0.1:{vnc_port}"));
    match cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(_) => Some(()),
        Err(e) => {
            warn!("websockify not available: {e}");
            None
        }
    }
}

fn xvfb_lock_path(display: &str) -> Option<std::path::PathBuf> {
    let n = display.strip_prefix(':')?;
    Some(std::path::PathBuf::from(format!("/tmp/.X{n}-lock")))
}

fn wait_for_xvfb(display: &str, timeout: Duration) -> Result<()> {
    let lock = xvfb_lock_path(display);
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if lock.as_ref().is_some_and(|p| p.exists()) && x11_display_ready(display) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    anyhow::bail!("Xvfb display {display} did not become ready in time");
}

fn x11_display_ready(display: &str) -> bool {
    match Command::new("xdpyinfo")
        .env("DISPLAY", display)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(s) if s.success() => true,
        Ok(_) => false,
        Err(_) => true,
    }
}

pub fn tcp_port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok()
}

fn wait_for_tcp_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if tcp_port_open(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

fn pick_port(preferred: u16) -> Result<u16> {
    use std::net::TcpListener;
    if TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return Ok(preferred);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
