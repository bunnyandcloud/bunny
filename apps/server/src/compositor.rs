use crate::state::AppState;
use anyhow::{anyhow, Result};
use font8x8::UnicodeFonts;
use std::io::Cursor;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub enum SnapshotTarget {
    Browser,
    Shell,
    All,
}

pub struct SnapshotResult {
    pub png: Vec<u8>,
    pub caption: String,
}

pub async fn capture_snapshot(
    state: &AppState,
    session_id: Uuid,
    target: SnapshotTarget,
    shell_name: Option<&str>,
) -> Result<SnapshotResult> {
    match target {
        SnapshotTarget::Browser => capture_browser(state, session_id).await,
        SnapshotTarget::Shell => capture_shell(state, session_id, shell_name),
        SnapshotTarget::All => {
            let shell = capture_shell(state, session_id, shell_name);
            let browser = capture_browser(state, session_id).await;
            match (shell, browser) {
                (Ok(s), Ok(b)) => {
                    let png = stack_images_vertical(&s.png, &b.png)?;
                    Ok(SnapshotResult {
                        png,
                        caption: format!("{} + browser", s.caption),
                    })
                }
                (Ok(s), Err(e)) => {
                    tracing::warn!("browser snapshot failed: {e}");
                    Ok(SnapshotResult {
                        png: s.png,
                        caption: format!("{} (browser unavailable: {e})", s.caption),
                    })
                }
                (Err(_), Ok(b)) => Ok(b),
                (Err(e), Err(_)) => Err(e),
            }
        }
    }
}

async fn capture_browser(state: &AppState, session_id: Uuid) -> Result<SnapshotResult> {
    let browser_id = find_browser_for_session(state, session_id)?;
    let cdp_port = state
        .browsers
        .get_cdp_port(browser_id)
        .ok_or_else(|| anyhow!("browser not running"))?;
    let png = cdp_screenshot_png(cdp_port).await.unwrap_or_else(|_| {
        render_text_png(
            &format!("Browser {browser_id}\nOpen Web UI → Browser → Stream for live view."),
            900,
            120,
        )
        .unwrap_or_default()
    });
    Ok(SnapshotResult {
        png,
        caption: "Browser snapshot".into(),
    })
}

fn append_discord_below_pane(pane: &str, discord: &str) -> String {
    let discord = discord.trim();
    if discord.is_empty() {
        return pane.to_string();
    }
    let pane = pane.trim_end();
    if pane.is_empty() {
        format!("{discord}\n")
    } else {
        format!("{pane}\n{discord}\n")
    }
}

fn capture_shell(
    state: &AppState,
    session_id: Uuid,
    shell_name: Option<&str>,
) -> Result<SnapshotResult> {
    let term_id = resolve_terminal(state, session_id, shell_name)?;
    let pane = state
        .terminals
        .capture_snapshot_text(term_id)
        .unwrap_or_default();
    let discord = crate::terminals::discord_transcript_for_snapshot(state, term_id);
    let text = append_discord_below_pane(&pane, &discord);
    let redacted = state.redactor.read().redact_text(&text);
    let clean = normalize_terminal_text(&redacted);
    let label = shell_name
        .map(str::to_string)
        .or_else(|| state.terminals.name(term_id))
        .unwrap_or_else(|| "active".into());
    let png = render_text_png(&clean, 900, 720)?;
    Ok(SnapshotResult {
        png,
        caption: format!("Shell snapshot - {label}"),
    })
}

fn find_browser_for_session(state: &AppState, session_id: Uuid) -> Result<Uuid> {
    state
        .browser_sessions
        .read()
        .iter()
        .find(|(_, sid)| **sid == session_id)
        .map(|(id, _)| *id)
        .ok_or_else(|| anyhow!("no browser session for stream session"))
}

fn resolve_terminal(
    state: &AppState,
    session_id: Uuid,
    shell_name: Option<&str>,
) -> Result<Uuid> {
    if let Some(name) = shell_name {
        let auth_db = state.auth.db();
        let db = auth_db.lock();
        for (term_id, sid) in state.terminal_sessions.read().iter() {
            if *sid != session_id {
                continue;
            }
            if let Ok(Some(row)) = db.get_terminal(*term_id) {
                if row.2 == name {
                    return Ok(*term_id);
                }
            }
        }
        return Err(anyhow!("shell not found: {name}"));
    }
    let live: Vec<Uuid> = state
        .terminal_sessions
        .read()
        .iter()
        .filter(|(_, sid)| **sid == session_id)
        .filter(|(id, _)| state.terminals.status(**id).is_some())
        .map(|(id, _)| *id)
        .collect();
    if live.len() == 1 {
        return Ok(live[0]);
    }
    if live.len() > 1 {
        return Ok(live[0]);
    }
    state
        .terminal_sessions
        .read()
        .iter()
        .find(|(_, sid)| **sid == session_id)
        .map(|(id, _)| *id)
        .ok_or_else(|| anyhow!("no terminal for session"))
}

async fn cdp_screenshot_png(cdp_port: u16) -> Result<Vec<u8>> {
    let script = crate::cdp_collector::sidecar_script_path()
        .and_then(|p| {
            let s = p.parent()?.join("screenshot.js");
            if s.exists() {
                Some(s)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("screenshot.js missing"))?;
    let output = tokio::process::Command::new("node")
        .arg(&script)
        .env("BUNNY_CDP_PORT", cdp_port.to_string())
        .output()
        .await?;
    if output.status.success() && output.stdout.len() > 100 {
        return Ok(output.stdout);
    }
    Err(anyhow!("cdp screenshot failed"))
}

const CELL_W: u32 = 8;
const CELL_H: u32 = 8;
const MARGIN: u32 = 10;
const LINE_GAP: u32 = 2;

pub fn render_text_png(text: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let mut img = image::RgbaImage::new(width, height);
    let bg = image::Rgba([24, 24, 28, 255]);
    let fg = image::Rgba([220, 220, 230, 255]);
    for p in img.pixels_mut() {
        *p = bg;
    }

    let max_cols = ((width.saturating_sub(2 * MARGIN)) / CELL_W) as usize;
    let max_lines = ((height.saturating_sub(2 * MARGIN)) / (CELL_H + LINE_GAP)) as usize;
    let all_lines: Vec<String> = text
        .lines()
        .map(|line| line.chars().take(max_cols).collect())
        .collect();
    let start = all_lines.len().saturating_sub(max_lines);
    let lines: Vec<String> = all_lines[start..].to_vec();

    for (row, line) in lines.iter().enumerate() {
        let y = MARGIN + (row as u32) * (CELL_H + LINE_GAP);
        for (col, ch) in line.chars().enumerate() {
            let x = MARGIN + (col as u32) * CELL_W;
            draw_glyph(&mut img, x, y, ch, fg);
        }
    }

    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut buf, image::ImageFormat::Png)?;
    Ok(buf.into_inner())
}

fn draw_glyph(img: &mut image::RgbaImage, x: u32, y: u32, ch: char, color: image::Rgba<u8>) {
    let glyph = glyph_for(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if (bits >> col) & 1 == 1 {
                let px = x + col as u32;
                let py = y + row as u32;
                if px < img.width() && py < img.height() {
                    img.put_pixel(px, py, color);
                }
            }
        }
    }
}

fn glyph_for(ch: char) -> [u8; 8] {
    font8x8::BASIC_FONTS
        .get(ch)
        .or_else(|| font8x8::LATIN_FONTS.get(ch))
        .or_else(|| font8x8::BASIC_FONTS.get('?'))
        .unwrap_or([0; 8])
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Collapse carriage-return overwrites (vim status line, progress bars).
pub fn normalize_terminal_text(text: &str) -> String {
    strip_ansi(text)
        .split('\n')
        .map(|line| line.rsplit('\r').next().unwrap_or(line).trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn stack_images_vertical(top: &[u8], bottom: &[u8]) -> Result<Vec<u8>> {
    let a = image::load_from_memory(top)?;
    let b = image::load_from_memory(bottom)?;
    let w = a.width().max(b.width());
    let h = a.height() + b.height();
    let mut out = image::RgbaImage::new(w, h);
    image::imageops::overlay(&mut out, &a, 0, 0);
    image::imageops::overlay(&mut out, &b, 0, i64::from(a.height()));
    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(out).write_to(&mut buf, image::ImageFormat::Png)?;
    Ok(buf.into_inner())
}
