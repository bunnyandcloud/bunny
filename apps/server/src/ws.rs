use crate::state::AppState;
use crate::terminals::scrollback_dir;
use axum::extract::ws::{Message, WebSocket};
use bunny_pty::protocol::{ReplayChunk, TerminalClientMsg, TerminalServerMsg};
use bunny_pty::{scrollback, tmux};
use futures::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub async fn handle_terminal_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    terminal_id: Uuid,
    can_write: bool,
    from_offset: Option<u64>,
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

    if let Some(target) = state.terminals.tmux_target(terminal_id) {
        let cwd = terminal_cwd(&state, terminal_id).unwrap_or_else(default_shell_cwd);
        let shell = &state.config.terminal.shell;
        let _ = tmux::ensure_shell_running(&target, &cwd, shell);
    }

    let from = from_offset.unwrap_or(0);
    let has_history = send_replay(&mut sender, &state, terminal_id, from).await;
    if !has_history {
        state.terminals.refresh_display(terminal_id);
    }

    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(client_msg) = serde_json::from_str::<TerminalClientMsg>(&text) {
                            match client_msg {
                                TerminalClientMsg::Input { data } if can_write => {
                                    let _ = state.terminals.write(terminal_id, &data);
                                }
                                TerminalClientMsg::Resize { cols, rows } if can_write => {
                                    let _ = state.terminals.resize(terminal_id, cols, rows);
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
                                        let shell = &state.config.terminal.shell;
                                        let _ = tmux::ensure_shell_running(&target, &cwd, shell);
                                    }
                                    let from = from_offset.unwrap_or(0);
                                    let has_history =
                                        send_replay(&mut sender, &state, terminal_id, from).await;
                                    if !has_history {
                                        state.terminals.refresh_display(terminal_id);
                                    }
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
                    Ok(data) => {
                        let msg = TerminalServerMsg::Output { data, offset: 0 };
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

/// Push scrollback to the client. Returns true when persisted history was included.
async fn send_replay(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    state: &AppState,
    terminal_id: Uuid,
    from_offset: u64,
) -> bool {
    let dir = scrollback_dir(state);
    let disk = if from_offset == 0 {
        scrollback::load(&dir, terminal_id)
    } else {
        None
    };

    state.terminals.hydrate_scrollback_from_disk(terminal_id);

    let mut chunks: Vec<ReplayChunk> = state
        .terminals
        .buffer_replay(terminal_id, from_offset)
        .map(|rows| {
            rows.into_iter()
                .map(|(offset, data)| ReplayChunk { offset, data })
                .collect()
        })
        .unwrap_or_default();

    let mut total: usize = chunks.iter().map(|c| c.data.len()).sum();

    if from_offset == 0 {
        if let Some(disk_text) = disk.clone() {
            if total < disk_text.len() / 2 {
                chunks = vec![ReplayChunk {
                    offset: 1,
                    data: disk_text,
                }];
                total = chunks.first().map(|c| c.data.len()).unwrap_or(0);
            }
        }
    }

    let has_history = total > 80;

    if has_history {
        tracing::info!(
            terminal = %terminal_id,
            bytes = total,
            disk_bytes = disk.as_ref().map(|d| d.len()).unwrap_or(0),
            "sending terminal history replay"
        );
    } else if from_offset == 0 {
        tracing::debug!(
            terminal = %terminal_id,
            disk_bytes = disk.as_ref().map(|d| d.len()).unwrap_or(0),
            buffer_bytes = total,
            "no terminal history to replay"
        );
    }

    if !chunks.is_empty() {
        let msg = TerminalServerMsg::Replay {
            chunks,
            has_history,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = sender.send(Message::Text(json)).await;
        }
    }

    has_history
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
