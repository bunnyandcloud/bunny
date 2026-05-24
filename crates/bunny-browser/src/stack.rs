use anyhow::Result;
use bunny_core::types::BrowserStatus;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserStackConfig {
    pub width: u32,
    pub height: u32,
    pub target_url: String,
    pub ephemeral_profile: bool,
}

pub struct BrowserStack {
    pub x11_display: String,
    pub cdp_port: u16,
    pub vnc_port: u16,
    pub novnc_port: u16,
    pub status: BrowserStatus,
}

impl BrowserStack {
    pub fn start(config: &BrowserStackConfig) -> Result<Self> {
        let display_num = 99u32;
        let x11_display = format!(":{display_num}");
        let cdp_port = pick_port(9222)?;
        let vnc_port = pick_port(5900)?;
        let novnc_port = pick_port(6080)?;

        info!(%x11_display, cdp_port, vnc_port, novnc_port, "starting browser stack");

        let xvfb = spawn_xvfb(&x11_display, config.width, config.height)?;
        std::thread::sleep(std::time::Duration::from_millis(500));

        let chromium = spawn_chromium(&x11_display, cdp_port, &config.target_url, config.ephemeral_profile)?;
        let vnc = spawn_vnc(&x11_display, vnc_port);
        let websockify = spawn_websockify(vnc_port, novnc_port);

        let _ = (xvfb, chromium, vnc, websockify);

        Ok(Self {
            x11_display,
            cdp_port,
            vnc_port,
            novnc_port,
            status: BrowserStatus::Running,
        })
    }

    pub fn stop(&mut self) {
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg(&self.x11_display)
            .status();
        self.status = BrowserStatus::Stopped;
    }

    pub fn is_running(&mut self) -> bool {
        self.status == BrowserStatus::Running
    }
}

fn spawn_xvfb(display: &str, width: u32, height: u32) -> Result<()> {
    let screen = format!("{width}x{height}x24");
    Command::new("Xvfb")
        .arg(display)
        .arg("-screen")
        .arg("0")
        .arg(&screen)
        .arg("-ac")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start Xvfb: {e}. Install xvfb package."))?;
    Ok(())
}

fn spawn_chromium(
    display: &str,
    cdp_port: u16,
    url: &str,
    ephemeral: bool,
) -> Result<()> {
    let mut cmd = Command::new("chromium");
    cmd.env("DISPLAY", display)
        .arg(format!("--remote-debugging-port={cdp_port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-gpu")
        .arg(url);
    if ephemeral {
        cmd.arg("--incognito");
    }
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .or_else(|_| {
            Command::new("google-chrome")
                .env("DISPLAY", display)
                .arg(format!("--remote-debugging-port={cdp_port}"))
                .arg(url)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| anyhow::anyhow!("failed to start chromium: {e}"))
        })?;
    Ok(())
}

fn spawn_vnc(display: &str, port: u16) -> Option<()> {
    match Command::new("x11vnc")
        .env("DISPLAY", display)
        .arg("-display")
        .arg(display)
        .arg("-rfbport")
        .arg(port.to_string())
        .arg("-localhost")
        .arg("-nopw")
        .arg("-forever")
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
    match Command::new("websockify")
        .arg(listen_port.to_string())
        .arg(format!("127.0.0.1:{vnc_port}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => Some(()),
        Err(e) => {
            warn!("websockify not available: {e}");
            None
        }
    }
}

fn pick_port(preferred: u16) -> Result<u16> {
    use std::net::TcpListener;
    if TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return Ok(preferred);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
