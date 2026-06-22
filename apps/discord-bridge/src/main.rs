mod threads;

use anyhow::Result;
use base64::Engine as _;
use bunny_i18n::{is_valid_locale_code, Locale, t};
use serenity::all::{
    Command, CommandDataOptionValue, CommandOptionType, CreateActionRow, CreateAttachment,
    CreateButton, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateMessage,
    EditInteractionResponse, Interaction, Ready,
};
use serenity::model::application::ButtonStyle;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

fn bunny_api_user_message(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = v.pointer("/error/message").and_then(|m| m.as_str()) {
            let trimmed = msg.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return format!("Request failed ({status})");
    }
    trimmed.to_string()
}

fn format_discord_error(locale: Locale, err: impl std::fmt::Display) -> String {
    format!("**{}**\n{}", t(locale, "discord.error.title", &[]), err)
}

#[derive(Clone, serde::Deserialize)]
struct BridgeConfig {
    discord: DiscordSection,
    bunny: BunnySection,
}

#[derive(Clone, serde::Deserialize)]
struct DiscordSection {
    application_id: u64,
    bot_token: String,
    /// When set, slash commands register on this guild instantly (recommended for dev).
    guild_id: Option<u64>,
}

#[derive(Clone, serde::Deserialize)]
struct BunnySection {
    internal_url: String,
    bridge_token: String,
    #[allow(dead_code)]
    public_url: Option<String>,
}

#[derive(Clone)]
struct BunnyClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl BunnyClient {
    fn new(base: &str, token: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    async fn post_json<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<serde_json::Value> {
        self.post_json_timeout(path, body, Duration::from_secs(30))
            .await
    }

    pub(crate) async fn post_json_timeout<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        let res = self
            .http
            .post(format!("{}/api/v1/internal/discord{path}", self.base))
            .timeout(timeout)
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            if status.as_u16() == 405 {
                anyhow::bail!(
                    "bunny API 405 Method Not Allowed on {path} — restart `bunny run` in the container after rebuilding (cargo build --release -p bunny-server)"
                );
            }
            anyhow::bail!("{}", bunny_api_user_message(status, &text));
        }
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::json!({ "raw": text })))
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<serde_json::Value> {
        let res = self
            .http
            .get(format!("{}/api/v1/internal/discord{path}", self.base))
            .bearer_auth(&self.token)
            .query(query)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("{}", bunny_api_user_message(status, &text));
        }
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::json!({ "raw": text })))
    }

    async fn post_snapshot(&self, body: &serde_json::Value) -> Result<SnapshotPayload> {
        let res = self
            .http
            .post(format!("{}/api/v1/internal/discord/snapshot", self.base))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().await?;
            anyhow::bail!("{}", bunny_api_user_message(status, &text));
        }
        let content_type = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if content_type.contains("application/json") {
            let v: serde_json::Value = res.json().await?;
            let format = v.get("format").and_then(|x| x.as_str()).unwrap_or("text");
            let caption = v
                .get("caption")
                .and_then(|x| x.as_str())
                .unwrap_or("Snapshot")
                .to_string();
            let text = v
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if format == "shell_text_and_browser" {
                let shell_caption = v
                    .get("shell_caption")
                    .and_then(|x| x.as_str())
                    .unwrap_or(&caption)
                    .to_string();
                let browser_png = v
                    .get("browser_png_base64")
                    .and_then(|x| x.as_str())
                    .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
                    .unwrap_or_default();
                let browser_unavailable = v
                    .get("browser_unavailable")
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                return Ok(SnapshotPayload::ShellTextAndBrowser {
                    caption,
                    shell_caption,
                    text,
                    browser_png,
                    browser_unavailable,
                });
            }
            return Ok(SnapshotPayload::Text { caption, text });
        }
        let caption = res
            .headers()
            .get("x-bunny-snapshot-caption")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("Snapshot")
            .to_string();
        let bytes = res.bytes().await?.to_vec();
        Ok(SnapshotPayload::Image {
            caption,
            bytes,
            filename: "snapshot.png".into(),
        })
    }

    async fn post_file(&self, body: &serde_json::Value) -> Result<(Vec<u8>, String, String)> {
        let res = self
            .http
            .post(format!("{}/api/v1/internal/discord/shell/file", self.base))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().await?;
            anyhow::bail!("{}", bunny_api_user_message(status, &text));
        }
        let caption = res
            .headers()
            .get("x-bunny-file-caption")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("File")
            .to_string();
        let filename = res
            .headers()
            .get("x-bunny-file-name")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("file.txt")
            .to_string();
        let bytes = res.bytes().await?.to_vec();
        Ok((bytes, filename, caption))
    }
}

struct Handler {
    bunny: BunnyClient,
    dev_guild_id: Option<u64>,
    bot_id: std::sync::Mutex<Option<serenity::model::id::UserId>>,
    thread_runtime: std::sync::Arc<threads::ThreadRuntime>,
}

enum SnapshotPayload {
    Text { caption: String, text: String },
    Image {
        caption: String,
        bytes: Vec<u8>,
        filename: String,
    },
    ShellTextAndBrowser {
        caption: String,
        shell_caption: String,
        text: String,
        browser_png: Vec<u8>,
        browser_unavailable: Option<String>,
    },
}

/// Discord reply payload (PNG stays in memory — never written to disk).
enum CommandReply {
    /// One or more chat messages (Discord limit 2000 chars each).
    Text(Vec<String>),
    /// Claude tool / shell approval — first page includes Allow/Deny buttons.
    PendingApproval {
        pages: Vec<String>,
        approval_id: String,
    },
    Snapshot {
        caption: String,
        png: Vec<u8>,
        filename: String,
    },
    /// Shell text (paginated) + browser PNG attachment (`full_snapshot`).
    FullSnapshot {
        text_pages: Vec<String>,
        png: Vec<u8>,
        filename: String,
        browser_note: String,
    },
    File {
        caption: String,
        bytes: Vec<u8>,
        filename: String,
    },
}

fn text_reply(content: impl Into<String>) -> CommandReply {
    CommandReply::Text(vec![content.into()])
}

/// Discord hard limit; stay slightly under for markdown edge cases.
const DISCORD_PAGE_LIMIT: usize = 1990;
const MAX_DISCORD_PAGES: usize = 10;

async fn ctx_from_interaction_resolved(
    http: &serenity::http::Http,
    cmd: &serenity::model::application::CommandInteraction,
) -> serde_json::Value {
    let parent = threads::parent_channel_id_for_id(http, cmd.channel_id).await;
    serde_json::json!({
        "guild_id": cmd.guild_id.map(|g| g.get().to_string()).unwrap_or_default(),
        "channel_id": parent,
        "thread_id": cmd.channel_id.get().to_string(),
        "discord_user_id": cmd.user.id.get().to_string(),
    })
}

fn query_ctx(ctx: &serde_json::Value) -> Vec<(String, String)> {
    let mut q = vec![
        ("guild_id".into(), ctx["guild_id"].as_str().unwrap_or("").into()),
        ("channel_id".into(), ctx["channel_id"].as_str().unwrap_or("").into()),
        (
            "discord_user_id".into(),
            ctx["discord_user_id"].as_str().unwrap_or("").into(),
        ),
    ];
    if let Some(thread_id) = ctx["thread_id"].as_str() {
        if !thread_id.is_empty() {
            q.push(("thread_id".into(), thread_id.into()));
        }
    }
    q
}

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        tracing::info!("discord bridge connected as {}", ready.user.name);
        if let Ok(mut id) = self.bot_id.lock() {
            *id = Some(ready.user.id);
        }
        let commands = vec![
            CreateCommand::new("bunny")
                .description("Control a linked Bunny session")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "link",
                        "Link channel to session",
                    )
                    .add_sub_option(
                        CreateCommandOption::new(CommandOptionType::String, "code", "Link code")
                            .required(true),
                    ),
                )
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "unlink",
                    "Unlink channel",
                ))
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "status",
                    "Link status",
                ))
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "language",
                        "Set UI language (en/fr)",
                    )
                    .add_sub_option(
                        CreateCommandOption::new(CommandOptionType::String, "locale", "en or fr")
                            .required(true)
                            .add_string_choice("English", "en")
                            .add_string_choice("French", "fr"),
                    ),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "snapshot",
                        "Shell output (last 50 lines, same as Web UI)",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "shell",
                        "Shell name (see shell_list; default = first shell)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "full_snapshot",
                        "Shell + browser snapshot (starts browser if needed)",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "shell",
                        "Shell name (see shell_list; default = first shell)",
                    ))
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "url",
                        "Browser URL (default: first preview port or http://127.0.0.1:3000)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "shell_list",
                        "List shells",
                    ),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "shell_new",
                        "Create a new shell",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "name",
                        "Shell name (default: next shell N)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "shell_close",
                        "Close a shell (kills tmux window)",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "shell",
                        "Shell name (required if more than one; see shell_list)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "run", "Run shell command")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "command", "Command")
                                .required(true),
                        )
                        .add_sub_option(
                            CreateCommandOption::new(
                                CommandOptionType::String,
                                "shell",
                                "Shell name (see shell_list; default = first shell)",
                            ),
                        ),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "run_stop",
                        "Stop foreground process in shell (Ctrl+C)",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "shell",
                        "Shell name (default: last used in this channel)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "file",
                        "Send a workspace file as Discord attachment (full file, up to 24 MB)",
                    )
                    .add_sub_option(
                        CreateCommandOption::new(CommandOptionType::String, "path", "Path relative to shell cwd")
                            .required(true),
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "shell",
                        "Shell name (default: last used in this channel)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "stream_browser_start",
                        "Start browser and post watch URL (read-only by default)",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "url",
                        "Browser URL (default: first preview port or http://127.0.0.1:3000)",
                    ))
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::Boolean,
                        "interactive",
                        "Allow mouse/keyboard on the watch link (default: read-only)",
                    ))
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::Integer,
                        "port",
                        "Local dev server port (default: first preview port or 3000)",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "stream_browser_stop",
                        "Stop browser watch stream(s) for this channel",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "url",
                        "Watch URL to stop (default: all active streams in this channel)",
                    )),
                )
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "stream_status",
                    "Watch status",
                ))
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "ask", "Ask Claude")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "prompt", "Question")
                                .required(true),
                        )
                        .add_sub_option(CreateCommandOption::new(
                            CommandOptionType::String,
                            "shell",
                            "Shell name (default: last used in this channel)",
                        )),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "plan", "Plan with Claude")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "prompt", "Task")
                                .required(true),
                        )
                        .add_sub_option(CreateCommandOption::new(
                            CommandOptionType::String,
                            "shell",
                            "Shell name (default: last used in this channel)",
                        )),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "do", "Do with Claude")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "prompt", "Task")
                                .required(true),
                        )
                        .add_sub_option(CreateCommandOption::new(
                            CommandOptionType::String,
                            "shell",
                            "Shell name (default: last used in this channel)",
                        )),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "stop", "Stop task")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "task_id", "Task id")
                                .required(true),
                        ),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "claude_reset",
                        "Reset Claude conversation for this Discord channel (ask/plan session)",
                    ),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "project",
                        "Show or set project directory for this channel",
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "path",
                        "Absolute path to project root",
                    )),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "git",
                        "Git commands in project directory",
                    )
                    .add_sub_option(
                        CreateCommandOption::new(
                            CommandOptionType::String,
                            "action",
                            "status|diff|log|checkout|branch|merge|reset_hard",
                        )
                        .required(true),
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "branch",
                        "Branch name (checkout/branch/merge)",
                    ))
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::String,
                        "path",
                        "File path for diff",
                    )),
                ),
        ];
        if let Err(e) = register_slash_commands(&ctx, &ready, self.dev_guild_id, commands).await {
            tracing::error!("register commands: {e}");
        }
    }

    async fn message(&self, ctx: Context, msg: serenity::model::channel::Message) {
        let bot_id = match self.bot_id.lock().ok().and_then(|g| *g) {
            Some(id) => id,
            None => return,
        };
        if let Err(e) = threads::handle_message(
            &ctx,
            &msg,
            &self.bunny,
            bot_id,
            &self.thread_runtime,
        )
        .await
        {
            tracing::error!("discord message handler: {e}");
        }
    }

    async fn reaction_add(&self, ctx: Context, reaction: serenity::model::channel::Reaction) {
        let bot_id = match self.bot_id.lock().ok().and_then(|g| *g) {
            Some(id) => id,
            None => return,
        };
        if reaction.user_id == Some(bot_id) {
            return;
        }
        if !threads::is_stop_emoji(&reaction.emoji) {
            return;
        }
        let parent_ch = if let Ok(ch) = reaction.channel_id.to_channel(&ctx.http).await {
            if let serenity::model::channel::Channel::Guild(g) = ch {
                if let Some(parent) = g.parent_id {
                    parent.get().to_string()
                } else {
                    reaction.channel_id.get().to_string()
                }
            } else {
                reaction.channel_id.get().to_string()
            }
        } else {
            reaction.channel_id.get().to_string()
        };
        let thread_id = reaction.channel_id.get().to_string();
        let user_id = reaction
            .user_id
            .map(|u| u.get().to_string())
            .unwrap_or_default();
        let bctx = threads::bridge_ctx(
            reaction.guild_id,
            &parent_ch,
            Some(&thread_id),
            &user_id,
        );
        if let Err(e) = threads::handle_stop_reaction(
            &ctx,
            &self.bunny,
            &thread_id,
            &bctx,
            reaction.message_id.get(),
        )
        .await
        {
            tracing::error!("stop reaction: {e}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let http = ctx.http.clone();
        let bunny = self.bunny.clone();

        match interaction {
            Interaction::Command(cmd) => {
                if cmd
                    .create_response(
                        &http,
                        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
                    )
                    .await
                    .is_err()
                {
                    tracing::error!("discord defer failed (gateway or token issue?)");
                    return;
                }
                tokio::spawn(async move {
                    let bridge_ctx = ctx_from_interaction_resolved(&http, &cmd).await;
                    let locale = user_locale(&bunny, &bridge_ctx).await;
                    let response = match handle_command(&bunny, &cmd, &bridge_ctx).await {
                        Ok(reply) => reply,
                        Err(e) => text_reply(format_discord_error(locale, &e)),
                    };
                    if let Err(e) = deliver_command_reply(&cmd, &http, response).await {
                        tracing::error!("discord reply failed: {e}");
                    }
                });
            }
            Interaction::Component(comp) => {
                if threads::parse_thread_permission_button(&comp.data.custom_id).is_some() {
                    let bunny = bunny.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            threads::handle_thread_permission_button(&comp, http.clone(), &bunny)
                                .await
                        {
                            tracing::error!("thread permission button: {e}");
                            let _ = comp
                                .create_followup(
                                    &http,
                                    CreateInteractionResponseFollowup::new()
                                        .content(format!("❌ Erreur : {e}")),
                                )
                                .await;
                        }
                    });
                    return;
                }
                if threads::parse_thread_question_button(&comp.data.custom_id).is_some() {
                    let bunny = bunny.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            threads::handle_thread_question_button(&comp, http.clone(), &bunny).await
                        {
                            tracing::error!("thread question button: {e}");
                            let _ = comp
                                .create_followup(
                                    &http,
                                    CreateInteractionResponseFollowup::new()
                                        .content(format!("❌ Erreur : {e}")),
                                )
                                .await;
                        }
                    });
                    return;
                }
                if threads::parse_thread_merge_button(&comp.data.custom_id).is_some() {
                    let bunny = bunny.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            threads::handle_thread_merge_button(&comp, &http, &bunny).await
                        {
                            tracing::error!("thread merge button: {e}");
                            let _ = comp
                                .create_followup(
                                    &http,
                                    CreateInteractionResponseFollowup::new()
                                        .content(format!("❌ Merge : {e}")),
                                )
                                .await;
                        }
                    });
                    return;
                }
                if let Some((goal, _thread_id)) = parse_goal_button(&comp.data.custom_id) {
                    let bunny = bunny.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            threads::handle_goal_cancel_button(&comp, &http, &bunny, goal).await
                        {
                            tracing::error!("goal/cancel: {e}");
                            let _ = comp
                                .create_followup(
                                    &http,
                                    CreateInteractionResponseFollowup::new()
                                        .content(format!("❌ Erreur : {e}")),
                                )
                                .await;
                        }
                    });
                    return;
                }
                let Some((approve, approval_id)) = parse_approval_button(&comp.data.custom_id)
                else {
                    return;
                };
                if comp
                    .create_response(
                        &http,
                        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
                    )
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::spawn(async move {
                    let bridge_ctx = serde_json::json!({
                        "guild_id": comp.guild_id.map(|g| g.get().to_string()).unwrap_or_default(),
                        "channel_id": comp.channel_id.get().to_string(),
                        "thread_id": comp.channel_id.get().to_string(),
                        "discord_user_id": comp.user.id.get().to_string(),
                    });
                    let loc = user_locale(&bunny, &bridge_ctx).await;
                    let mut body = bridge_ctx;
                    body["approval_id"] = serde_json::json!(approval_id);
                    body["approve"] = serde_json::json!(approve);
                    let result = bunny
                        .post_json_timeout("/approval/resolve", &body, Duration::from_secs(180))
                        .await;
                    let followup = match result {
                        Ok(res) => {
                            if !approve {
                                text_reply(t(loc, "discord.approval.denied", &[]))
                            } else if res.get("output").and_then(|v| v.as_str()).is_some() {
                                let mode = res
                                    .get("mode")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("do");
                                let shell = res
                                    .get("shell")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("shell");
                                CommandReply::Text(format_agent_reply_pages(&serde_json::json!({
                                    "mode": mode,
                                    "shell": shell,
                                    "exit_code": res.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0),
                                    "output": res.get("output").and_then(|v| v.as_str()).unwrap_or(""),
                                })))
                            } else {
                                text_reply(t(loc, "discord.approval.approved", &[]))
                            }
                        }
                        Err(e) => text_reply(format_discord_error(loc, &e)),
                    };
                    if let Err(e) =
                        deliver_component_followup(&comp, &http, followup, &approval_id).await
                    {
                        tracing::error!("approval followup failed: {e}");
                    }
                });
            }
            _ => {}
        }
    }
}

async fn run_snapshot(
    bunny: &BunnyClient,
    bridge_ctx: &serde_json::Value,
    sub_opts: &[serenity::all::CommandDataOption],
    target: &str,
    ensure_browser: bool,
) -> Result<CommandReply> {
    let mut body = bridge_ctx.clone();
    body["target"] = serde_json::json!(target);
    if ensure_browser {
        body["ensure_browser"] = serde_json::json!(true);
    }
    if let Some(shell) = opt_str(sub_opts, "shell") {
        body["shell_name"] = serde_json::json!(shell);
    }
    if let Some(url) = opt_str(sub_opts, "url") {
        body["browser_url"] = serde_json::json!(url);
    }
    match bunny.post_snapshot(&body).await? {
        SnapshotPayload::Text { caption, text } => Ok(format_shell_snapshot_reply(&caption, &text)),
        SnapshotPayload::ShellTextAndBrowser {
            caption,
            shell_caption,
            text,
            browser_png,
            browser_unavailable,
        } => Ok(format_full_snapshot_reply(
            &shell_caption,
            &text,
            &caption,
            browser_png,
            browser_unavailable.as_deref(),
        )),
        SnapshotPayload::Image {
            caption,
            bytes,
            filename,
        } => Ok(CommandReply::Snapshot {
            caption: format!("{caption} (not stored on disk)."),
            png: bytes,
            filename,
        }),
    }
}

pub(crate) async fn user_locale(bunny: &BunnyClient, bridge_ctx: &serde_json::Value) -> Locale {
    let q = query_ctx(bridge_ctx);
    let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    for path in ["/status", "/user-locale"] {
        if let Ok(res) = bunny.get_json(path, &q_ref).await {
            if let Some(l) = res.get("locale").and_then(|v| v.as_str()) {
                return Locale::from_db(l);
            }
        }
    }
    Locale::En
}

async fn handle_command(
    bunny: &BunnyClient,
    cmd: &serenity::model::application::CommandInteraction,
    bridge_ctx: &serde_json::Value,
) -> Result<CommandReply> {
    let locale = user_locale(bunny, bridge_ctx).await;
    if cmd.data.name != "bunny" {
        return Ok(text_reply(t(locale, "discord.unknown_command", &[])));
    }
    let (sub_name, sub_opts) = subcommand_opts(cmd)?;
    match sub_name.as_str() {
        "link" => {
            let code = opt_str(&sub_opts, "code").ok_or_else(|| anyhow::anyhow!("code required"))?;
            let mut body = bridge_ctx.clone();
            body["code"] = serde_json::json!(code);
            let res = bunny.post_json("/link", &body).await?;
            Ok(text_reply(t(
                locale,
                "discord.link.success",
                &[(
                    "session_id",
                    res.get("session_id").and_then(|v| v.as_str()).unwrap_or("?"),
                )],
            )))
        }
        "unlink" => {
            bunny.post_json("/unlink", bridge_ctx).await?;
            Ok(text_reply(t(locale, "discord.unlink.success", &[])))
        }
        "language" => {
            let loc_str = opt_str(&sub_opts, "locale").unwrap_or("");
            if !is_valid_locale_code(loc_str) {
                return Ok(text_reply(t(locale, "discord.language.invalid", &[])));
            }
            let mut body = bridge_ctx.clone();
            body["locale"] = serde_json::json!(loc_str);
            let res = bunny.post_json("/locale", &body).await;
            match res {
                Ok(v) => {
                    let msg = v
                        .get("message")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            t(
                                Locale::from_db(loc_str),
                                "discord.language.updated",
                                &[("locale", loc_str)],
                            )
                        });
                    Ok(text_reply(msg))
                }
                Err(e) => {
                    let hint = if e.to_string().contains("403")
                        || e.to_string().contains("discord_not_linked")
                    {
                        t(locale, "discord.language.not_linked", &[])
                    } else {
                        format_discord_error(locale, &e)
                    };
                    Ok(text_reply(hint))
                }
            }
        }
        "status" => {
            let q = query_ctx(bridge_ctx);
            let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let res = bunny.get_json("/status", &q_ref).await?;
            Ok(text_reply(format!(
                "```json\n{}\n```",
                serde_json::to_string_pretty(&res)?
            )))
        }
        "snapshot" => run_snapshot(bunny, bridge_ctx, &sub_opts, "shell", false).await,
        "full_snapshot" => run_snapshot(bunny, bridge_ctx, &sub_opts, "all", true).await,
        "shell_list" => {
            let q = query_ctx(bridge_ctx);
            let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let res = bunny.get_json("/shell/list", &q_ref).await?;
            let mut lines = vec![t(locale, "discord.shell_list.title", &[])];
            if let Some(items) = res.as_array() {
                if items.is_empty() {
                    lines.push(t(locale, "discord.shell_list.empty", &[]));
                }
                for item in items {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                    let default = item.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
                    let tag = if default {
                        t(locale, "discord.shell_list.default_tag", &[])
                    } else {
                        String::new()
                    };
                    lines.push(format!("• `{name}` ({status}){tag}"));
                }
                lines.push(t(locale, "discord.shell_list.hint_run", &[]));
                lines.push(t(locale, "discord.shell_list.hint_manage", &[]));
            }
            Ok(text_reply(lines.join("\n")))
        }
        "shell_new" => {
            let mut body = bridge_ctx.clone();
            if let Some(name) = opt_str(&sub_opts, "name") {
                body["name"] = serde_json::json!(name);
            }
            let res = bunny.post_json("/shell/new", &body).await?;
            let name = res.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let id = res
                .get("terminal_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            Ok(text_reply(t(
                locale,
                "discord.shell_new.created",
                &[("name", name), ("id", id)],
            )))
        }
        "shell_close" => {
            let mut body = bridge_ctx.clone();
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let res = bunny.post_json("/shell/close", &body).await?;
            let name = res.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            Ok(text_reply(t(locale, "discord.shell_close.closed", &[("name", name)])))
        }
        "run_stop" => {
            let mut body = bridge_ctx.clone();
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let res = bunny.post_json("/shell/run/stop", &body).await?;
            let shell = res
                .get("shell")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            Ok(text_reply(t(locale, "discord.run_stop.body", &[("shell", shell)])))
        }
        "run" | "shell_run" => {
            let command = opt_str(&sub_opts, "command").unwrap_or("");
            let mut body = bridge_ctx.clone();
            body["command"] = serde_json::json!(command);
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let res = bunny
                .post_json_timeout("/shell/run", &body, Duration::from_secs(55))
                .await?;
            if let Some(output) = res.get("output").and_then(|v| v.as_str()) {
                let exit_code = res.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0);
                let persistent = res
                    .get("persistent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let shell = res
                    .get("shell")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                let shell_auto_created = res
                    .get("shell_auto_created")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let previous_shell = res
                    .get("previous_shell")
                    .and_then(|v| v.as_str());
                let text = if output.trim().is_empty() {
                    t(locale, "discord.run.no_output", &[])
                } else {
                    output.trim().to_string()
                };
                if persistent {
                    Ok(format_persistent_shell_run_reply(
                        locale,
                        shell,
                        command,
                        &text,
                        shell_auto_created,
                        previous_shell,
                    ))
                } else {
                    Ok(format_shell_run_reply(
                        locale,
                        shell,
                        command,
                        &text,
                        exit_code,
                        shell_auto_created,
                        previous_shell,
                    ))
                }
            } else {
                Ok(text_reply(format!(
                    "```json\n{}\n```",
                    serde_json::to_string_pretty(&res)?
                )))
            }
        }
        "file" => {
            let path = opt_str(&sub_opts, "path").unwrap_or("");
            let mut body = bridge_ctx.clone();
            body["path"] = serde_json::json!(path);
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let (bytes, filename, caption) = bunny.post_file(&body).await?;
            Ok(CommandReply::File {
                caption,
                bytes,
                filename,
            })
        }
        "stream_browser_start" => {
            let mut body = bridge_ctx.clone();
            if let Some(url) = opt_str(&sub_opts, "url") {
                body["browser_url"] = serde_json::json!(url);
            }
            if let Some(port) = opt_integer(&sub_opts, "port") {
                if !(1..=65535).contains(&port) {
                    return Ok(text_reply("Invalid port: must be 1–65535."));
                }
                body["browser_port"] = serde_json::json!(port as u16);
            }
            if let Some(interactive) = opt_bool(&sub_opts, "interactive") {
                body["interactive"] = serde_json::json!(interactive);
            }
            let res = bunny.post_json("/stream/start", &body).await?;
            let url = res.get("watch_url").and_then(|v| v.as_str()).unwrap_or("?");
            let mode = res.get("mode").and_then(|v| v.as_str()).unwrap_or("read_only");
            let label = if mode == "interactive" {
                "Interactive watch (read + write)"
            } else {
                "Live read-only watch"
            };
            Ok(text_reply(format!("Browser started. {label}:\n{url}")))
        }
        "stream_browser_stop" => {
            let watch_url = opt_str(&sub_opts, "url");
            let mut body = bridge_ctx.clone();
            if let Some(url) = watch_url {
                body["url"] = serde_json::json!(url);
            }
            let res = bunny.post_json("/stream/stop", &body).await?;
            let stopped = res.get("stopped").and_then(|v| v.as_u64()).unwrap_or(0);
            if watch_url.is_some() {
                Ok(text_reply(if stopped > 0 {
                    "Watch stream stopped."
                } else {
                    "No matching active watch stream for that URL."
                }))
            } else if stopped > 0 {
                Ok(text_reply(format!(
                    "Stopped {stopped} browser watch stream(s)."
                )))
            } else {
                Ok(text_reply("No active browser watch streams."))
            }
        }
        "stream_status" => {
            let q = query_ctx(bridge_ctx);
            let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let res = bunny.get_json("/stream/status", &q_ref).await?;
            Ok(text_reply(format!(
                "```json\n{}\n```",
                serde_json::to_string_pretty(&res)?
            )))
        }
        "ask" | "plan" | "do" => {
            let prompt = opt_str(&sub_opts, "prompt").unwrap_or("");
            let path = format!("/agent/{sub_name}");
            let mut body = bridge_ctx.clone();
            body["agent"] = serde_json::json!("claude");
            body["prompt"] = serde_json::json!(prompt);
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let res = bunny
                .post_json_timeout(&path, &body, Duration::from_secs(180))
                .await?;
            if res.get("needs_approval").and_then(|v| v.as_bool()) == Some(true) {
                return Ok(format_approval_reply(locale, &res));
            }
            Ok(CommandReply::Text(format_agent_reply_pages(&res)))
        }
        "claude_reset" => {
            bunny.post_json("/claude/reset", bridge_ctx).await?;
            Ok(text_reply(t(locale, "discord.claude_reset.done", &[])))
        }
        "project" => {
            let path = opt_str(&sub_opts, "path");
            threads::run_project_command(bunny, bridge_ctx, path, locale).await
        }
        "git" => {
            let action = opt_str(&sub_opts, "action").unwrap_or("status");
            let branch = opt_str(&sub_opts, "branch");
            let path = opt_str(&sub_opts, "path");
            threads::run_git_command(bunny, bridge_ctx, action, branch, path, locale).await
        }
        "stop" => {
            let task_id = opt_str(&sub_opts, "task_id").unwrap_or("");
            let mut body = bridge_ctx.clone();
            body["task_id"] = serde_json::json!(task_id);
            bunny.post_json("/task/stop", &body).await?;
            Ok(text_reply("Task stop requested."))
        }
        _ => Ok(text_reply(t(
            locale,
            "discord.unknown_subcommand",
            &[("name", &sub_name)],
        ))),
    }
}

fn subcommand_opts(
    cmd: &serenity::model::application::CommandInteraction,
) -> Result<(String, Vec<serenity::all::CommandDataOption>)> {
    let top = cmd
        .data
        .options
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing subcommand"))?;
    let name = top.name.clone();
    match &top.value {
        CommandDataOptionValue::SubCommand(opts) => Ok((name, opts.clone())),
        _ => Err(anyhow::anyhow!("expected subcommand")),
    }
}

fn opt_str<'a>(opts: &'a [serenity::all::CommandDataOption], name: &str) -> Option<&'a str> {
    opts.iter()
        .find(|o| o.name == name)
        .and_then(|o| match &o.value {
            CommandDataOptionValue::String(s) => Some(s.as_str()),
            _ => None,
        })
}

fn approval_button_rows(approval_id: &str) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("bunny:approve:{approval_id}"))
            .label("Autoriser")
            .style(ButtonStyle::Success),
        CreateButton::new(format!("bunny:deny:{approval_id}"))
            .label("Refuser")
            .style(ButtonStyle::Danger),
    ])]
}

fn parse_goal_button(custom_id: &str) -> Option<(bool, String)> {
    custom_id
        .strip_prefix("bunny:goal:")
        .map(|id| (true, id.to_string()))
        .or_else(|| {
            custom_id
                .strip_prefix("bunny:cancel:")
                .map(|id| (false, id.to_string()))
        })
}

pub(crate) fn paginate_plain(text: &str) -> Vec<String> {
    const MAX: usize = 1990;
    if text.len() <= MAX {
        return vec![text.to_string()];
    }
    let mut pages = Vec::new();
    let mut rest = text.to_string();
    while !rest.is_empty() {
        if rest.len() <= MAX {
            pages.push(rest);
            break;
        }
        let split = rest[..MAX].rfind('\n').unwrap_or(MAX);
        pages.push(rest[..split].to_string());
        rest = rest[split..].trim_start().to_string();
    }
    pages
}

fn parse_approval_button(custom_id: &str) -> Option<(bool, String)> {
    custom_id
        .strip_prefix("bunny:approve:")
        .map(|id| (true, id.to_string()))
        .or_else(|| {
            custom_id
                .strip_prefix("bunny:deny:")
                .map(|id| (false, id.to_string()))
        })
}

async fn deliver_component_followup(
    comp: &serenity::model::application::ComponentInteraction,
    http: &serenity::http::Http,
    reply: CommandReply,
    approval_id: &str,
) -> Result<()> {
    let _ = comp
        .edit_response(
            http,
            EditInteractionResponse::new()
                .content(if reply_is_denied(&reply) {
                    "Refusé."
                } else {
                    "Traitement…"
                })
                .components(vec![]),
        )
        .await;

    match reply {
        CommandReply::Text(pages) => {
            for page in cap_discord_pages(Locale::En, pages) {
                comp.create_followup(
                    http,
                    CreateInteractionResponseFollowup::new().content(page),
                )
                .await?;
            }
        }
        other => {
            deliver_command_reply_as_followup(comp, http, other).await?;
        }
    }
    let _ = approval_id;
    Ok(())
}

fn reply_is_denied(reply: &CommandReply) -> bool {
    matches!(
        reply,
        CommandReply::Text(p) if p.first().map(|s| s.contains("Refusé")).unwrap_or(false)
    )
}

async fn deliver_command_reply_as_followup(
    comp: &serenity::model::application::ComponentInteraction,
    http: &serenity::http::Http,
    reply: CommandReply,
) -> Result<()> {
    match reply {
        CommandReply::Text(pages) => {
            for page in cap_discord_pages(Locale::En, pages) {
                comp.create_followup(
                    http,
                    CreateInteractionResponseFollowup::new().content(page),
                )
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn deliver_command_reply(
    cmd: &serenity::model::application::CommandInteraction,
    http: &serenity::http::Http,
    reply: CommandReply,
) -> Result<()> {
    match reply {
        CommandReply::Text(pages) => {
            let pages = cap_discord_pages(Locale::En, pages);
            let Some(first) = pages.first() else {
                cmd.edit_response(http, EditInteractionResponse::new().content("(empty)"))
                    .await?;
                return Ok(());
            };
            if let Err(e) = cmd
                .edit_response(http, EditInteractionResponse::new().content(first))
                .await
            {
                tracing::error!("discord reply failed: {e}");
                let short = first.chars().take(1800).collect::<String>();
                let _ = cmd
                    .edit_response(
                        http,
                        EditInteractionResponse::new().content(format!(
                            "{short}\n\n_(réponse tronquée — erreur Discord: {e})_"
                        )),
                    )
                    .await;
                return Ok(());
            }
            for page in pages.iter().skip(1) {
                if let Err(e) = cmd
                    .channel_id
                    .send_message(http, CreateMessage::new().content(page))
                    .await
                {
                    tracing::error!("discord follow-up page failed: {e}");
                }
            }
        }
        CommandReply::PendingApproval { pages, approval_id } => {
            let pages = cap_discord_pages(Locale::En, pages);
            let Some(first) = pages.first() else {
                cmd.edit_response(http, EditInteractionResponse::new().content("(empty)"))
                    .await?;
                return Ok(());
            };
            cmd.edit_response(
                http,
                EditInteractionResponse::new()
                    .content(first)
                    .components(approval_button_rows(&approval_id)),
            )
            .await?;
            for page in pages.iter().skip(1) {
                cmd.channel_id
                    .send_message(http, CreateMessage::new().content(page))
                    .await?;
            }
        }
        CommandReply::Snapshot {
            caption,
            png,
            filename,
        } => {
            cmd.edit_response(
                http,
                EditInteractionResponse::new()
                    .content(caption)
                    .new_attachment(CreateAttachment::bytes(png, filename)),
            )
            .await?;
        }
        CommandReply::FullSnapshot {
            text_pages,
            png,
            filename,
            browser_note,
        } => {
            let pages = cap_discord_pages(Locale::En, text_pages);
            let Some(first) = pages.first() else {
                cmd.edit_response(http, EditInteractionResponse::new().content(&browser_note))
                    .await?;
                return Ok(());
            };
            let content = format!("{first}\n\n{browser_note}");
            cmd.edit_response(
                http,
                EditInteractionResponse::new()
                    .content(content)
                    .new_attachment(CreateAttachment::bytes(png, filename)),
            )
            .await?;
            for page in pages.iter().skip(1) {
                cmd.channel_id
                    .send_message(http, CreateMessage::new().content(page))
                    .await?;
            }
        }
        CommandReply::File {
            caption,
            bytes,
            filename,
        } => {
            cmd.edit_response(
                http,
                EditInteractionResponse::new()
                    .content(caption)
                    .new_attachment(CreateAttachment::bytes(bytes, filename)),
            )
            .await?;
        }
    }
    Ok(())
}

fn cap_discord_pages(locale: Locale, mut pages: Vec<String>) -> Vec<String> {
    if pages.len() <= MAX_DISCORD_PAGES {
        return pages;
    }
    pages.truncate(MAX_DISCORD_PAGES);
    if let Some(last) = pages.last_mut() {
        last.push_str("\n\n");
        last.push_str(&t(locale, "discord.run.web_ui_footer", &[]));
    }
    pages
}

/// `/bunny full_snapshot`: shell text like `snapshot` + browser PNG attachment.
fn format_full_snapshot_reply(
    shell_caption: &str,
    text: &str,
    full_caption: &str,
    browser_png: Vec<u8>,
    browser_unavailable: Option<&str>,
) -> CommandReply {
    let browser_note = browser_unavailable
        .map(|e| format!("{full_caption}\nBrowser unavailable: {e}"))
        .unwrap_or_else(|| format!("{full_caption}\nBrowser screenshot attached (not stored on disk)."));

    if browser_png.is_empty() {
        let mut reply = format_shell_snapshot_reply(shell_caption, text);
        if let CommandReply::Text(ref mut pages) = reply {
            if let Some(last) = pages.last_mut() {
                last.push_str("\n\n");
                last.push_str(&browser_note);
            }
        }
        return reply;
    }

    let shell = format_shell_snapshot_reply(shell_caption, text);
    let text_pages = match shell {
        CommandReply::Text(pages) => pages,
        _ => vec!["(empty shell)".into()],
    };
    CommandReply::FullSnapshot {
        text_pages,
        png: browser_png,
        filename: "browser.png".into(),
        browser_note,
    }
}

/// `/bunny snapshot`: header + fenced scrollback tail (paginated for Discord limits).
fn format_shell_snapshot_reply(caption: &str, text: &str) -> CommandReply {
    let body = if text.trim().is_empty() {
        "(empty shell)".to_string()
    } else {
        let mut page = String::new();
        page.push_str("```\n");
        page.push_str(text);
        if !text.ends_with('\n') {
            page.push('\n');
        }
        page.push_str("```");
        page
    };
    let page = format!("{caption}\n{body}");
    CommandReply::Text(paginate_plain(&page))
}

/// Long-running `/bunny run`: prose header + single fenced excerpt (no nested markdown).
fn format_persistent_shell_run_reply(
    locale: Locale,
    shell: &str,
    command: &str,
    excerpt: &str,
    shell_auto_created: bool,
    previous_shell: Option<&str>,
) -> CommandReply {
    let mut header = t(
        locale,
        "discord.run.persistent_header",
        &[("shell", shell), ("command", command)],
    );
    if shell_auto_created {
        if let Some(prev) = previous_shell {
            header.push('\n');
            header.push_str(&t(
                locale,
                "discord.run.shell_auto_created",
                &[("previous", prev), ("shell", shell)],
            ));
        }
    }
    let no_out = t(locale, "discord.run.no_output", &[]);
    let body = if excerpt.trim().is_empty() || excerpt == no_out.as_str() {
        t(locale, "discord.run.no_output_yet", &[])
    } else {
        let mut page = String::new();
        page.push_str("```\n");
        page.push_str(excerpt);
        if !excerpt.ends_with('\n') {
            page.push('\n');
        }
        page.push_str("```");
        page
    };
    let page = format!("{header}\n{body}");
    CommandReply::Text(cap_discord_pages(locale, vec![page]))
}

/// Paginate shell output so each Discord message stays under 2000 chars.
fn format_shell_run_reply(
    locale: Locale,
    shell: &str,
    command: &str,
    text: &str,
    exit_code: i64,
    shell_auto_created: bool,
    previous_shell: Option<&str>,
) -> CommandReply {
    let suffix = if exit_code == 0 {
        String::new()
    } else {
        format!(
            "\n{}",
            t(
                locale,
                "discord.shell_run.exit",
                &[("code", &exit_code.to_string())]
            )
        )
    };
    let mut header = t(
        locale,
        "discord.shell_run.header",
        &[("shell", shell), ("command", command)],
    );
    if shell_auto_created {
        if let Some(prev) = previous_shell {
            header.push('\n');
            header.push_str(&t(
                locale,
                "discord.run.shell_auto_created",
                &[("previous", prev), ("shell", shell)],
            ));
        }
    }
    // Leave room for header, fences, and "(suite N/M)" on follow-up messages.
    let chunk_budget = 1700usize;
    let chunks = if text.contains("```") {
        split_discord_text_respecting_fences(text, chunk_budget)
    } else {
        split_discord_text_plain(text, chunk_budget)
    };
    let chunks = if chunks.is_empty() {
        vec![text.to_string()]
    } else {
        chunks
    };
    let total = chunks.len();
    let mut pages = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut page = String::new();
        if i == 0 {
            page.push_str(&header);
            if total > 1 {
                page.push_str(&format!(" · (1/{total})"));
            }
            page.push_str("\n\n");
        } else {
            page.push_str(&format!(
                "{}\n\n",
                t(
                    locale,
                    "discord.shell_run.continuation",
                    &[
                        ("current", &(i + 1).to_string()),
                        ("total", &total.to_string()),
                    ],
                )
            ));
        }
        page.push_str("```\n");
        page.push_str(chunk);
        page.push_str("\n```");
        if i + 1 == total {
            page.push_str(&suffix);
        }
        if page.len() <= DISCORD_PAGE_LIMIT {
            pages.push(page);
        } else {
            // Safety: split an oversized page (very long line).
            for sub in split_discord_text_plain(&page, DISCORD_PAGE_LIMIT) {
                pages.push(sub);
            }
        }
    }
    CommandReply::Text(cap_discord_pages(locale, pages))
}

fn format_approval_reply(locale: Locale, res: &serde_json::Value) -> CommandReply {
    let approval_id = res
        .get("approval_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    let mode = res.get("mode").and_then(|v| v.as_str()).unwrap_or("agent");
    let shell = res.get("shell").and_then(|v| v.as_str()).unwrap_or("shell");
    let summary = res
        .get("summary")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| t(locale, "discord.approval.summary_default", &[]));
    let header = t(
        locale,
        "discord.approval.header",
        &[("mode", mode), ("shell", shell)],
    );
    let body = format_claude_markdown_for_discord(&summary);
    let footer = format!("\n\n{}", t(locale, "discord.approval.footer", &[]));
    let pages = paginate_discord_reply(&header, &body, &footer);
    CommandReply::PendingApproval {
        pages,
        approval_id,
    }
}

fn format_agent_reply_pages(res: &serde_json::Value) -> Vec<String> {
    let mode = res.get("mode").and_then(|v| v.as_str()).unwrap_or("agent");
    let shell = res.get("shell").and_then(|v| v.as_str()).unwrap_or("default");
    let exit_code = res.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0);
    let output = res
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let body = if output.is_empty() {
        "_(no output)_".to_string()
    } else {
        format_claude_markdown_for_discord(output)
    };
    let suffix = if exit_code == 0 {
        String::new()
    } else {
        format!("\n\n_(exit {exit_code})_")
    };
    let header = format!("**Claude {mode}** · shell `{shell}`");
    paginate_discord_reply(&header, &body, &suffix)
}

/// Split body across Discord messages; header on first page only, suffix on last.
fn paginate_discord_reply(header: &str, body: &str, suffix: &str) -> Vec<String> {
    let first_budget = DISCORD_PAGE_LIMIT.saturating_sub(header.len() + 4);
    let mut body_pages = split_discord_text(body, first_budget.max(400));
    if body_pages.is_empty() {
        body_pages.push(String::new());
    }

    let mut out = Vec::new();
    let total = body_pages.len();
    for (i, chunk) in body_pages.iter().enumerate() {
        let mut page = String::new();
        if i == 0 {
            page.push_str(header);
            if total > 1 {
                page.push_str(&format!(" · (1/{total})"));
            }
            page.push_str("\n\n");
        } else {
            page.push_str(&format!("(suite {}/{total})\n\n", i + 1));
        }
        page.push_str(chunk);
        if i + 1 == total {
            page.push_str(&suffix);
        }
        if page.len() > DISCORD_PAGE_LIMIT {
            let overflow = split_discord_text(&page, DISCORD_PAGE_LIMIT);
            out.extend(overflow);
        } else {
            out.push(page);
        }
    }
    if out.is_empty() {
        out.push(format!("{header}\n\n_(no output){suffix}"));
    }
    out
}

fn split_discord_text(text: &str, max: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }
    if text.contains("```") {
        return split_discord_text_respecting_fences(text, max);
    }
    split_discord_text_plain(text, max)
}

fn split_discord_text_plain(text: &str, max: usize) -> Vec<String> {
    if text.len() <= max {
        return vec![text.to_string()];
    }
    let mut rest = text.to_string();
    let mut pages = Vec::new();
    while !rest.is_empty() {
        if rest.len() <= max {
            pages.push(rest);
            break;
        }
        let byte_limit = rest
            .char_indices()
            .take_while(|(i, _)| *i < max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max);
        let mut split_at = rest[..byte_limit].rfind("\n\n").or_else(|| rest[..byte_limit].rfind('\n'));
        if split_at.unwrap_or(0) < max / 4 {
            split_at = Some(byte_limit);
        }
        let at = split_at.unwrap_or(byte_limit).max(1);
        let chunk = rest[..at].trim_end().to_string();
        if !chunk.is_empty() {
            pages.push(chunk);
        }
        rest = rest[at..].trim_start().to_string();
    }
    pages
}

/// Split long replies without breaking Markdown code fences (Discord needs closed ``` per message).
fn split_discord_text_respecting_fences(text: &str, max: usize) -> Vec<String> {
    if text.len() <= max {
        return vec![text.to_string()];
    }

    let mut pages = Vec::new();
    let mut page = String::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();

    let flush = |page: &mut String, pages: &mut Vec<String>| {
        let trimmed = page.trim_end().to_string();
        if !trimmed.is_empty() {
            pages.push(trimmed);
        }
        page.clear();
    };

    let close_fence = |page: &mut String| {
        if !page.ends_with('\n') {
            page.push('\n');
        }
        page.push_str("```\n");
    };

    let open_fence = |page: &mut String, lang: &str| {
        page.push_str("```");
        page.push_str(lang);
        page.push('\n');
    };

    for line in text.lines() {
        let trimmed = line.trim();
        let is_fence_line = trimmed.starts_with("```");
        let line_nl = format!("{line}\n");

        if is_fence_line {
            if !in_fence {
                if !page.is_empty() && page.len() + line_nl.len() > max {
                    flush(&mut page, &mut pages);
                }
                in_fence = true;
                fence_lang = trimmed.trim_start_matches('`').to_string();
                if page.len() + line_nl.len() > max && !page.is_empty() {
                    flush(&mut page, &mut pages);
                }
                page.push_str(&line_nl);
            } else {
                in_fence = false;
                if page.len() + line_nl.len() > max && !page.is_empty() {
                    flush(&mut page, &mut pages);
                }
                page.push_str(&line_nl);
                fence_lang.clear();
            }
            continue;
        }

        if page.len() + line_nl.len() > max && !page.is_empty() {
            if in_fence {
                close_fence(&mut page);
                flush(&mut page, &mut pages);
                open_fence(&mut page, &fence_lang);
            } else {
                flush(&mut page, &mut pages);
            }
        }
        page.push_str(&line_nl);
    }

    if in_fence {
        close_fence(&mut page);
    }
    flush(&mut page, &mut pages);

    if pages.is_empty() {
        return split_discord_text_plain(text, max);
    }

    // Second pass: any page still over limit (very long single lines) gets plain-split.
    let mut final_pages = Vec::new();
    for p in pages {
        if p.len() <= max {
            final_pages.push(p);
        } else {
            final_pages.extend(split_discord_text_plain(&p, max));
        }
    }
    final_pages
}

/// Map common Claude markdown to Discord message markdown (no wrapping code fence).
fn format_claude_markdown_for_discord(text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim().starts_with("```") {
            in_fence = !in_fence;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_fence {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            out.push_str("**");
            out.push_str(rest.trim());
            out.push_str("**\n");
        } else if let Some(rest) = line.strip_prefix("## ") {
            out.push_str("**");
            out.push_str(rest.trim());
            out.push_str("**\n");
        } else if let Some(rest) = line.strip_prefix("# ") {
            out.push_str("**");
            out.push_str(rest.trim());
            out.push_str("**\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

fn opt_bool(opts: &[serenity::all::CommandDataOption], name: &str) -> Option<bool> {
    opts.iter()
        .find(|o| o.name == name)
        .and_then(|o| match &o.value {
            CommandDataOptionValue::Boolean(b) => Some(*b),
            _ => None,
        })
}

fn opt_integer(opts: &[serenity::all::CommandDataOption], name: &str) -> Option<i64> {
    opts.iter()
        .find(|o| o.name == name)
        .and_then(|o| match &o.value {
            CommandDataOptionValue::Integer(n) => Some(*n),
            _ => None,
        })
}

async fn register_slash_commands(
    ctx: &Context,
    ready: &Ready,
    dev_guild_id: Option<u64>,
    commands: Vec<CreateCommand>,
) -> serenity::Result<()> {
    if let Some(guild_id) = dev_guild_id {
        let cleared = clear_global_slash_commands(ctx).await?;
        if cleared > 0 {
            tracing::info!(cleared, "removed stale global slash commands");
        }
        GuildId::new(guild_id)
            .set_commands(&ctx.http, commands)
            .await?;
        tracing::info!(
            guild_id,
            "registered guild slash commands (visible immediately in this server)"
        );
    } else {
        let cleared_guilds = clear_guild_slash_commands(ctx, ready, None).await?;
        if cleared_guilds > 0 {
            tracing::info!(
                cleared_guilds,
                "removed stale guild slash commands (avoids duplicate /bunny entries with global registration)"
            );
        }
        Command::set_global_commands(&ctx.http, commands).await?;
        tracing::info!(
            "registered global slash commands — set discord.guild_id in bridge config for instant dev updates"
        );
    }
    Ok(())
}

/// Remove guild-scoped slash commands so they do not stack with global `/bunny` in Discord autocomplete.
async fn clear_guild_slash_commands(
    ctx: &Context,
    ready: &Ready,
    except_guild_id: Option<u64>,
) -> serenity::Result<usize> {
    let mut cleared = 0usize;
    for guild in &ready.guilds {
        let id = guild.id.get();
        if except_guild_id == Some(id) {
            continue;
        }
        let guild_id = GuildId::new(id);
        let existing = guild_id.get_commands(&ctx.http).await.unwrap_or_default();
        if existing.is_empty() {
            continue;
        }
        guild_id.set_commands(&ctx.http, vec![]).await?;
        cleared += 1;
        tracing::debug!(guild_id = id, count = existing.len(), "cleared guild slash commands");
    }
    Ok(cleared)
}

async fn clear_global_slash_commands(ctx: &Context) -> serenity::Result<usize> {
    let existing = Command::get_global_commands(&ctx.http).await?;
    let n = existing.len();
    for cmd in existing {
        Command::delete_global_command(&ctx.http, cmd.id).await?;
    }
    let _ = Command::set_global_commands(&ctx.http, vec![]).await;
    Ok(n)
}

fn load_config() -> Result<BridgeConfig> {
    if let (Ok(bot_token), Ok(app_id), Ok(internal_url), Ok(bridge_token)) = (
        std::env::var("DISCORD_BOT_TOKEN"),
        std::env::var("DISCORD_APPLICATION_ID"),
        std::env::var("BUNNY_INTERNAL_URL"),
        std::env::var("BUNNY_BRIDGE_TOKEN"),
    ) {
        let application_id: u64 = app_id
            .parse()
            .map_err(|_| anyhow::anyhow!("DISCORD_APPLICATION_ID must be numeric"))?;
        return Ok(BridgeConfig {
            discord: DiscordSection {
                application_id,
                bot_token,
                guild_id: std::env::var("DISCORD_GUILD_ID")
                    .ok()
                    .and_then(|s| s.parse().ok()),
            },
            bunny: BunnySection {
                internal_url,
                bridge_token,
                public_url: std::env::var("BUNNY_PUBLIC_URL").ok(),
            },
        });
    }

    let path = std::env::var("BUNNY_DISCORD_BRIDGE_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(format!("{home}/.config/bunny/discord-bridge.yaml"))
        });
    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    Ok(serde_yaml::from_str(&text)?)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("bunny_discord_bridge=info".parse()?),
        )
        .init();

    let config = load_config()?;
    let bunny = BunnyClient::new(&config.bunny.internal_url, &config.bunny.bridge_token);

    // GUILDS: slash commands; MESSAGE_CONTENT: @bunny mentions (privileged — enable in Developer Portal)
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGE_REACTIONS;
    let mut client = Client::builder(&config.discord.bot_token, intents)
        .event_handler(Handler {
            bunny,
            dev_guild_id: config.discord.guild_id,
            bot_id: std::sync::Mutex::new(None),
            thread_runtime: std::sync::Arc::new(threads::ThreadRuntime::new()),
        })
        .application_id(config.discord.application_id.into())
        .await?;

    client.start().await?;
    Ok(())
}
