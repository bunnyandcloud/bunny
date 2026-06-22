use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use bunny_pty::protocol::{ReplayChunk, ReplayMode, TerminalClientMsg, TerminalServerMsg};
use bunny_pty::tmux;
use futures::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message as UpstreamMessage};
use uuid::Uuid;

struct ReplayResponse {
    replay_mode: ReplayMode,
    snapshot_offset: u64,
    chunks: Vec<ReplayChunk>,
}

pub async fn handle_terminal_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    terminal_id: Uuid,
    can_write: bool,
    user_id: Uuid,
    _from_offset: Option<u64>,
) {
    let (mut sender, mut receiver) = socket.split();

    let Some(mut out_rx) = state.terminals.subscribe(terminal_id) else {
        let msg = TerminalServerMsg::Error {
            code: "terminal_unavailable".into(),
            message: "This shell is no longer running. Close it with × and open a new shell."
                .into(),
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = sender.send(Message::Text(json)).await;
        }
        return;
    };

    state
        .git_identity
        .on_attach(terminal_id, user_id, can_write);

    if let Some(target) = state.terminals.tmux_target(terminal_id) {
        let cwd = terminal_cwd(&state, terminal_id).unwrap_or_else(default_shell_cwd);
        let session_env = tmux_env_for_terminal(&state, terminal_id);
        let shell_cmd = interactive_shell_cmd(&state, terminal_id, &session_env);
        let _ = tmux::ensure_shell_running(&target, &cwd, &shell_cmd, &session_env);
        tmux::configure_pane_for_web(&target);
    }

    let mut live_fence: u64 = 0;
    let mut pending_refresh_after_resize = false;
    let mut line_buf = String::new();

    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(client_msg) = serde_json::from_str::<TerminalClientMsg>(&text) {
                            match client_msg {
                                TerminalClientMsg::Input { data } if can_write => {
                                    let refresh = handle_terminal_input(
                                        &state,
                                        terminal_id,
                                        user_id,
                                        &data,
                                        &mut line_buf,
                                    );
                                    if refresh {
                                        crate::terminal_context_watch::schedule_context_refresh_after_input(
                                            state.clone(),
                                            terminal_id,
                                        );
                                    }
                                }
                                TerminalClientMsg::Resize { cols, rows } if can_write => {
                                    let _ = state.terminals.resize(terminal_id, cols, rows);
                                    if pending_refresh_after_resize {
                                        pending_refresh_after_resize = false;
                                        state.terminals.refresh_display(terminal_id);
                                    }
                                }
                                TerminalClientMsg::Ping { id } => {
                                    let pong = TerminalServerMsg::Pong { id };
                                    if let Ok(json) = serde_json::to_string(&pong) {
                                        let _ = sender.send(Message::Text(json)).await;
                                    }
                                }
                                TerminalClientMsg::Subscribe { from_offset } => {
                                    if let Some(target) = state.terminals.tmux_target(terminal_id) {
                                        let cwd = terminal_cwd(&state, terminal_id)
                                            .unwrap_or_else(default_shell_cwd);
                                        let session_env = tmux_env_for_terminal(&state, terminal_id);
                                        let shell_cmd =
                                            interactive_shell_cmd(&state, terminal_id, &session_env);
                                        let _ = tmux::ensure_shell_running(
                                            &target,
                                            &cwd,
                                            &shell_cmd,
                                            &session_env,
                                        );
                                        tmux::configure_pane_for_web(&target);
                                    }
                                    let from = from_offset.unwrap_or(0);
                                    let replay = build_replay(&state, terminal_id, from);
                                    live_fence = replay.snapshot_offset;
                                    pending_refresh_after_resize =
                                        replay.replay_mode == ReplayMode::None;
                                    send_replay(&mut sender, replay).await;
                                }
                                TerminalClientMsg::Refresh => {
                                    state.terminals.refresh_display(terminal_id);
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
            out = out_rx.recv() => {
                match out {
                    Ok(chunk) => {
                        if chunk.offset <= live_fence {
                            continue;
                        }
                        let msg = TerminalServerMsg::Output {
                            data: chunk.data,
                            offset: chunk.offset,
                        };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if sender.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

fn build_replay(state: &AppState, terminal_id: Uuid, from_offset: u64) -> ReplayResponse {
    let disk_merged = crate::terminals::load_scrollback_for_replay(state, terminal_id);
    let disk_has_discord = disk_merged.contains("[discord]");

    if let Some(text) = state.terminals.take_recovery_replay(terminal_id) {
        let snapshot = state
            .terminals
            .buffer_offset(terminal_id)
            .unwrap_or(0);
        if text.trim().is_empty() {
            return ReplayResponse {
                replay_mode: ReplayMode::None,
                snapshot_offset: snapshot,
                chunks: vec![],
            };
        }
        tracing::info!(
            terminal = %terminal_id,
            bytes = text.len(),
            "sending terminal recovery replay"
        );
        return ReplayResponse {
            replay_mode: ReplayMode::Recovery,
            snapshot_offset: snapshot,
            chunks: vec![ReplayChunk {
                offset: 1,
                data: text,
            }],
        };
    }

    let snapshot = state
        .terminals
        .buffer_offset(terminal_id)
        .unwrap_or(0);

    if from_offset >= snapshot {
        if from_offset == 0 && disk_has_discord && !disk_merged.trim().is_empty() {
            let data = disk_merged.replace('\n', "\r\n");
            tracing::info!(
                terminal = %terminal_id,
                bytes = data.len(),
                "sending discord transcript replay from disk"
            );
            return ReplayResponse {
                replay_mode: ReplayMode::Recovery,
                snapshot_offset: snapshot,
                chunks: vec![ReplayChunk {
                    offset: 1,
                    data,
                }],
            };
        }
        return ReplayResponse {
            replay_mode: ReplayMode::None,
            snapshot_offset: snapshot,
            chunks: vec![],
        };
    }

    let rows = state
        .terminals
        .buffer_replay_range(terminal_id, from_offset, snapshot)
        .unwrap_or_default();

    if rows.is_empty() {
        if from_offset == 0 && disk_has_discord && !disk_merged.trim().is_empty() {
            let data = disk_merged.replace('\n', "\r\n");
            return ReplayResponse {
                replay_mode: ReplayMode::Recovery,
                snapshot_offset: snapshot,
                chunks: vec![ReplayChunk {
                    offset: 1,
                    data,
                }],
            };
        }
        return ReplayResponse {
            replay_mode: ReplayMode::None,
            snapshot_offset: snapshot,
            chunks: vec![],
        };
    }

    let buffer_text: String = rows.iter().map(|(_, d)| d.as_str()).collect();
    if from_offset == 0 && disk_has_discord && !disk_merged.contains(&buffer_text) {
        let data = disk_merged.replace('\n', "\r\n");
        tracing::info!(
            terminal = %terminal_id,
            bytes = data.len(),
            "sending merged disk replay (discord transcript)"
        );
        return ReplayResponse {
            replay_mode: ReplayMode::Recovery,
            snapshot_offset: snapshot,
            chunks: vec![ReplayChunk {
                offset: 1,
                data,
            }],
        };
    }

    let total: usize = rows.iter().map(|(_, data)| data.len()).sum();
    tracing::info!(
        terminal = %terminal_id,
        bytes = total,
        from = from_offset,
        snapshot = snapshot,
        "sending terminal catch-up replay"
    );

    ReplayResponse {
        replay_mode: ReplayMode::CatchUp,
        snapshot_offset: snapshot,
        chunks: rows
            .into_iter()
            .map(|(offset, data)| ReplayChunk { offset, data })
            .collect(),
    }
}

async fn send_replay(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    replay: ReplayResponse,
) {
    let has_history = replay.replay_mode == ReplayMode::Recovery;
    let msg = TerminalServerMsg::Replay {
        chunks: replay.chunks,
        replay_mode: replay.replay_mode,
        snapshot_offset: replay.snapshot_offset,
        has_history,
    };
    if let Ok(json) = serde_json::to_string(&msg) {
        let _ = sender.send(Message::Text(json)).await;
    }
}

fn default_shell_cwd() -> PathBuf {
    crate::terminals::default_shell_cwd()
}

fn terminal_cwd(state: &AppState, terminal_id: Uuid) -> Option<PathBuf> {
    state
        .auth
        .db()
        .lock()
        .get_terminal(terminal_id)
        .ok()
        .flatten()
        .map(|row| PathBuf::from(row.5))
}

fn secret_env_for_terminal(state: &AppState, terminal_id: Uuid) -> std::collections::HashMap<String, String> {
    state
        .terminal_sessions
        .read()
        .get(&terminal_id)
        .map(|session_id| state.secret_env_for_session(*session_id))
        .unwrap_or_default()
}

fn tmux_env_for_terminal(
    state: &AppState,
    terminal_id: Uuid,
) -> std::collections::HashMap<String, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let mut env = state
        .git_identity
        .terminal_session_env(terminal_id, &home);
    env.extend(secret_env_for_terminal(state, terminal_id));
    env
}

fn interactive_shell_cmd(
    state: &AppState,
    terminal_id: Uuid,
    session_env: &std::collections::HashMap<String, String>,
) -> String {
    tmux::interactive_shell_command(
        std::path::Path::new(&state.data_dir),
        terminal_id,
        &state.config.terminal.shell,
        session_env,
    )
    .unwrap_or_else(|_| state.config.terminal.shell.clone())
}

fn handle_terminal_input(
    state: &AppState,
    terminal_id: Uuid,
    user_id: Uuid,
    data: &str,
    line_buf: &mut String,
) -> bool {
    state.git_identity.note_input(terminal_id, user_id);

    let mut forward = String::new();
    let mut submitted_line: Option<String> = None;
    for ch in data.chars() {
        if ch == '\x03' {
            line_buf.clear();
            if crate::terminals::send_terminal_interrupt(state, terminal_id).is_ok() {
                continue;
            }
            forward.push(ch);
        } else if ch == '\r' || ch == '\n' {
            let line = line_buf.trim().to_string();
            submitted_line = Some(line.clone());
            match line.as_str() {
                "bunny git use-me" => {
                    state.git_identity.pin_user(terminal_id, user_id);
                    let _ = state.terminals.write(
                        terminal_id,
                        "\r\nbunny git: identity pinned to your account.\r\n",
                    );
                }
                "bunny git whoami" => {
                    let msg = state
                        .git_identity
                        .whoami_message(state, terminal_id, user_id);
                    let _ = state.terminals.write(terminal_id, &msg);
                }
                _ => forward.push(ch),
            }
            line_buf.clear();
        } else if ch == '\u{7f}' || ch == '\u{8}' {
            line_buf.pop();
            forward.push(ch);
        } else if ch.is_control() {
            forward.push(ch);
        } else {
            line_buf.push(ch);
            forward.push(ch);
        }
    }

    if !forward.is_empty() {
        let _ = state.terminals.write(terminal_id, &forward);
    }

    submitted_line
        .as_deref()
        .is_some_and(crate::terminal_context_watch::input_may_change_context)
}

pub async fn handle_session_realtime(socket: WebSocket, state: Arc<AppState>, session_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();
    let mut hub_rx = state.realtime.subscribe(session_id);

    let welcome = serde_json::json!({
        "type": "session.status.changed",
        "sessionId": session_id.to_string(),
        "status": "ready"
    });
    if sender
        .send(Message::Text(welcome.to_string()))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if v.get("type").and_then(|t| t.as_str()) == Some("sync") {
                                let since = v.get("lastEventId").and_then(|x| x.as_u64()).unwrap_or(0);
                                let missed = fetch_timeline_since(&state, session_id, since);
                                let reply = serde_json::json!({
                                    "type": "sync.reply",
                                    "missedEvents": missed,
                                    "requiresBrowserReconnect": false
                                });
                                let _ = sender.send(Message::Text(reply.to_string())).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            hub_msg = hub_rx.recv() => {
                match hub_msg {
                    Ok(json) => {
                        if sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let notice = serde_json::json!({
                            "type": "recovery.degraded",
                            "detail": format!("missed {n} events")
                        });
                        if sender.send(Message::Text(notice.to_string())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

fn fetch_timeline_since(
    state: &AppState,
    session_id: Uuid,
    since: u64,
) -> Vec<serde_json::Value> {
    let auth_db = state.auth.db();
    let db = auth_db.lock();
    db.list_timeline(session_id, since, 200)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, source, event_type, payload, sequence, ts)| {
            serde_json::json!({
                "type": "timeline.event",
                "eventId": id.to_string(),
                "source": source,
                "eventType": event_type,
                "payload": serde_json::from_str::<serde_json::Value>(&payload).unwrap_or(serde_json::Value::Null),
                "sequence": sequence,
                "ts": ts,
            })
        })
        .collect()
}

/// Bidirectional proxy: browser noVNC client ↔ local websockify (Chromium desktop).
pub async fn handle_novnc_proxy(
    socket: WebSocket,
    novnc_port: u16,
    revoked: Option<Arc<AtomicBool>>,
) {
    let upstream_url = format!("ws://127.0.0.1:{novnc_port}/");
    proxy_websocket(socket, upstream_url, revoked).await;
}

pub async fn proxy_websocket(
    socket: WebSocket,
    upstream_url: String,
    revoked: Option<Arc<AtomicBool>>,
) {
    let Ok((upstream, _)) = connect_async(&upstream_url).await else {
        return;
    };

    let (mut client_tx, mut client_rx) = socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    let client_to_upstream = async {
        while let Some(msg) = client_rx.next().await {
            if revoked
                .as_ref()
                .is_some_and(|f| f.load(Ordering::SeqCst))
            {
                break;
            }
            let Ok(msg) = msg else { break };
            let upstream_msg = match msg {
                Message::Text(text) => UpstreamMessage::Text(text),
                Message::Binary(data) => UpstreamMessage::Binary(data),
                Message::Ping(data) => UpstreamMessage::Ping(data),
                Message::Pong(data) => UpstreamMessage::Pong(data),
                Message::Close(frame) => {
                    let close = frame.map(|f| UpstreamMessage::Close(Some(
                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(f.code),
                            reason: f.reason,
                        },
                    )));
                    let _ = upstream_tx.send(close.unwrap_or(UpstreamMessage::Close(None))).await;
                    break;
                }
            };
            if upstream_tx.send(upstream_msg).await.is_err() {
                break;
            }
        }
    };

    let upstream_to_client = async {
        while let Some(msg) = upstream_rx.next().await {
            if revoked
                .as_ref()
                .is_some_and(|f| f.load(Ordering::SeqCst))
            {
                let _ = client_tx
                    .send(Message::Close(None))
                    .await;
                break;
            }
            let Ok(msg) = msg else { break };
            let client_msg = match msg {
                UpstreamMessage::Text(text) => Message::Text(text),
                UpstreamMessage::Binary(data) => Message::Binary(data),
                UpstreamMessage::Ping(data) => Message::Ping(data),
                UpstreamMessage::Pong(data) => Message::Pong(data),
                UpstreamMessage::Close(frame) => {
                    let close = frame.map(|f| Message::Close(Some(
                        axum::extract::ws::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason,
                        },
                    )));
                    let _ = client_tx
                        .send(close.unwrap_or(Message::Close(None)))
                        .await;
                    break;
                }
                UpstreamMessage::Frame(_) => continue,
            };
            if client_tx.send(client_msg).await.is_err() {
                break;
            }
        }
    };

    let revoke_wait = async {
        if let Some(flag) = &revoked {
            while !flag.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        } else {
            std::future::pending::<()>().await
        }
    };

    tokio::select! {
        () = client_to_upstream => {}
        () = upstream_to_client => {}
        () = revoke_wait => {}
    }
}

pub async fn handle_browser_events(socket: WebSocket, state: Arc<AppState>, browser_id: Uuid) {
    let session_id = state
        .browser_sessions
        .read()
        .get(&browser_id)
        .copied();

    let Some(session_id) = session_id else {
        let (mut sender, mut receiver) = socket.split();
        let _ = sender
            .send(Message::Text(
                serde_json::json!({"type":"error","message":"browser session not linked"}).to_string(),
            ))
            .await;
        while receiver.next().await.is_some() {}
        return;
    };

    handle_session_realtime(socket, state, session_id).await;
}
