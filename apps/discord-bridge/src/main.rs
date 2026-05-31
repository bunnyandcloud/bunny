use anyhow::Result;
use serenity::all::{
    Command, CommandDataOptionValue, CommandOptionType, CreateAttachment, CreateCommand,
    CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse, Interaction, Ready,
};
use serenity::model::id::GuildId;
use serenity::prelude::*;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

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

    async fn post_json_timeout<T: serde::Serialize>(
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
            anyhow::bail!("bunny API {status}: {text}");
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
            anyhow::bail!("bunny API {status}: {text}");
        }
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::json!({ "raw": text })))
    }

    async fn post_snapshot(&self, body: &serde_json::Value) -> Result<(Vec<u8>, String, String)> {
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
            anyhow::bail!("snapshot failed {status}: {text}");
        }
        let caption = res
            .headers()
            .get("x-bunny-snapshot-caption")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("Snapshot")
            .to_string();
        let bytes = res.bytes().await?.to_vec();
        Ok((bytes, "snapshot.png".into(), caption))
    }
}

struct Handler {
    bunny: BunnyClient,
    dev_guild_id: Option<u64>,
}

/// Discord reply payload (PNG stays in memory — never written to disk).
enum CommandReply {
    Text(String),
    Snapshot {
        caption: String,
        png: Vec<u8>,
        filename: String,
    },
}

fn ctx_from_interaction(cmd: &serenity::model::application::CommandInteraction) -> serde_json::Value {
    serde_json::json!({
        "guild_id": cmd.guild_id.map(|g| g.get().to_string()).unwrap_or_default(),
        "channel_id": cmd.channel_id.get().to_string(),
        "thread_id": cmd.channel_id.get().to_string(),
        "discord_user_id": cmd.user.id.get().to_string(),
    })
}

fn query_ctx(ctx: &serde_json::Value) -> Vec<(String, String)> {
    vec![
        ("guild_id".into(), ctx["guild_id"].as_str().unwrap_or("").into()),
        ("channel_id".into(), ctx["channel_id"].as_str().unwrap_or("").into()),
        (
            "discord_user_id".into(),
            ctx["discord_user_id"].as_str().unwrap_or("").into(),
        ),
    ]
}

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        tracing::info!("discord bridge connected as {}", ready.user.name);
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
                        "snapshot",
                        "Shell snapshot (tmux pane)",
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
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "stream_start",
                    "Start watch link",
                ))
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "stream_stop",
                    "Stop watch",
                ))
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
                        ),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "plan", "Plan with Claude")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "prompt", "Task")
                                .required(true),
                        ),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "do", "Do with Claude")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "prompt", "Task")
                                .required(true),
                        ),
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::SubCommand, "stop", "Stop task")
                        .add_sub_option(
                            CreateCommandOption::new(CommandOptionType::String, "task_id", "Task id")
                                .required(true),
                        ),
                ),
        ];
        if let Err(e) = register_slash_commands(&ctx, self.dev_guild_id, commands).await {
            tracing::error!("register commands: {e}");
        }
    }

    async fn message(&self, ctx: Context, msg: serenity::model::channel::Message) {
        if msg.author.bot {
            return;
        }
        let Some(content) = msg.content.strip_prefix("@bunny") else {
            return;
        };
        let trimmed = content.trim();
        let lower = trimmed.to_lowercase();
        let (path, prompt) = if let Some(rest) = lower.strip_prefix("and claude do :") {
            ("/agent/do", rest.trim())
        } else if let Some(rest) = lower.strip_prefix("and claude ask :") {
            ("/agent/ask", rest.trim())
        } else if let Some(rest) = lower.strip_prefix("and claude plan :") {
            ("/agent/plan", rest.trim())
        } else {
            return;
        };
        if prompt.is_empty() {
            return;
        }
        let bridge_ctx = serde_json::json!({
            "guild_id": msg.guild_id.map(|g| g.get().to_string()).unwrap_or_default(),
            "channel_id": msg.channel_id.get().to_string(),
            "thread_id": msg.channel_id.get().to_string(),
            "discord_user_id": msg.author.id.get().to_string(),
        });
        let mut body = bridge_ctx;
        body["agent"] = serde_json::json!("claude");
        body["prompt"] = serde_json::json!(prompt);
        match self.bunny.post_json(path, &body).await {
            Ok(res) => {
                let _ = msg
                    .channel_id
                    .say(
                        &ctx.http,
                        format!(
                            "Task started: `{}`",
                            res.get("task_id").and_then(|v| v.as_str()).unwrap_or("?")
                        ),
                    )
                    .await;
            }
            Err(e) => {
                let _ = msg
                    .channel_id
                    .say(&ctx.http, format!("Error: {e}"))
                    .await;
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(cmd) = interaction else {
            return;
        };
        let http = ctx.http.clone();
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

        let bunny = self.bunny.clone();
        tokio::spawn(async move {
            let bridge_ctx = ctx_from_interaction(&cmd);
            let response = match handle_command(&bunny, &cmd, &bridge_ctx).await {
                Ok(reply) => reply,
                Err(e) => CommandReply::Text(format!("Error: {e}")),
            };
            let edit = match response {
                CommandReply::Text(content) => EditInteractionResponse::new().content(content),
                CommandReply::Snapshot {
                    caption,
                    png,
                    filename,
                } => EditInteractionResponse::new()
                    .content(caption)
                    .new_attachment(CreateAttachment::bytes(png, filename)),
            };
            if let Err(e) = cmd.edit_response(&http, edit).await {
                tracing::error!("discord edit_response failed: {e}");
            }
        });
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
    let (png, filename, caption) = bunny.post_snapshot(&body).await?;
    Ok(CommandReply::Snapshot {
        caption: format!("{caption} (not stored on disk)."),
        png,
        filename,
    })
}

async fn handle_command(
    bunny: &BunnyClient,
    cmd: &serenity::model::application::CommandInteraction,
    bridge_ctx: &serde_json::Value,
) -> Result<CommandReply> {
    if cmd.data.name != "bunny" {
        return Ok(CommandReply::Text("Unknown command".into()));
    }
    let (sub_name, sub_opts) = subcommand_opts(cmd)?;
    match sub_name.as_str() {
        "link" => {
            let code = opt_str(&sub_opts, "code").ok_or_else(|| anyhow::anyhow!("code required"))?;
            let mut body = bridge_ctx.clone();
            body["code"] = serde_json::json!(code);
            let res = bunny.post_json("/link", &body).await?;
            Ok(CommandReply::Text(format!(
                "Linked to Bunny session `{}`",
                res.get("session_id").and_then(|v| v.as_str()).unwrap_or("?")
            )))
        }
        "unlink" => {
            bunny.post_json("/unlink", bridge_ctx).await?;
            Ok(CommandReply::Text("Channel unlinked.".into()))
        }
        "status" => {
            let q = query_ctx(bridge_ctx);
            let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let res = bunny.get_json("/status", &q_ref).await?;
            Ok(CommandReply::Text(format!(
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
            let mut lines = vec!["**Shells**".into()];
            if let Some(items) = res.as_array() {
                if items.is_empty() {
                    lines.push("_No shell — open one in the Web UI first._".into());
                }
                for item in items {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                    let default = item.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
                    let tag = if default { " _(default)_" } else { "" };
                    lines.push(format!("• `{name}` ({status}){tag}"));
                }
                lines.push("Use `/bunny run shell:<name> command:...`".into());
                lines.push("`/bunny shell_new` · `/bunny shell_close shell:<name>`".into());
            }
            Ok(CommandReply::Text(lines.join("\n")))
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
            Ok(CommandReply::Text(format!(
                "Shell **`{name}`** created (`{id}`).\nOpen the Web UI Terminal tab to interact."
            )))
        }
        "shell_close" => {
            let mut body = bridge_ctx.clone();
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let res = bunny.post_json("/shell/close", &body).await?;
            let name = res.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            Ok(CommandReply::Text(format!("Shell **`{name}`** closed.")))
        }
        "run" | "shell_run" => {
            let command = opt_str(&sub_opts, "command").unwrap_or("");
            let mut body = bridge_ctx.clone();
            body["command"] = serde_json::json!(command);
            if let Some(shell) = opt_str(&sub_opts, "shell") {
                body["shell_name"] = serde_json::json!(shell);
            }
            let res = bunny
                .post_json_timeout("/shell/run", &body, Duration::from_secs(45))
                .await?;
            if let Some(output) = res.get("output").and_then(|v| v.as_str()) {
                let exit_code = res.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0);
                let shell = res
                    .get("shell")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                let text = if output.trim().is_empty() {
                    "(no output)".to_string()
                } else {
                    output.trim().to_string()
                };
                let suffix = if exit_code == 0 {
                    String::new()
                } else {
                    format!("\n(exit {exit_code})")
                };
                Ok(CommandReply::Text(format!(
                    "**Shell:** `{shell}`\n```\n{text}{suffix}\n```"
                )))
            } else {
                Ok(CommandReply::Text(format!(
                    "```json\n{}\n```",
                    serde_json::to_string_pretty(&res)?
                )))
            }
        }
        "stream_start" => {
            let res = bunny.post_json("/stream/start", bridge_ctx).await?;
            let url = res.get("watch_url").and_then(|v| v.as_str()).unwrap_or("?");
            Ok(CommandReply::Text(format!("Live Bunny watch (read-only):\n{url}")))
        }
        "stream_stop" => {
            bunny.post_json("/stream/stop", bridge_ctx).await?;
            Ok(CommandReply::Text("Watch stream stopped.".into()))
        }
        "stream_status" => {
            let q = query_ctx(bridge_ctx);
            let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let res = bunny.get_json("/stream/status", &q_ref).await?;
            Ok(CommandReply::Text(format!(
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
            let res = bunny.post_json(&path, &body).await?;
            Ok(CommandReply::Text(format!(
                "Task started: `{}`",
                res.get("task_id").and_then(|v| v.as_str()).unwrap_or("?")
            )))
        }
        "stop" => {
            let task_id = opt_str(&sub_opts, "task_id").unwrap_or("");
            let mut body = bridge_ctx.clone();
            body["task_id"] = serde_json::json!(task_id);
            bunny.post_json("/task/stop", &body).await?;
            Ok(CommandReply::Text("Task stop requested.".into()))
        }
        _ => Ok(CommandReply::Text(format!("Unknown subcommand: {}", sub_name))),
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

async fn register_slash_commands(
    ctx: &Context,
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
        Command::set_global_commands(&ctx.http, commands).await?;
        tracing::info!(
            "registered global slash commands — Discord clients may take up to 1 hour to refresh; set discord.guild_id in bridge config for instant dev updates"
        );
    }
    Ok(())
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
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&config.discord.bot_token, intents)
        .event_handler(Handler {
            bunny,
            dev_guild_id: config.discord.guild_id,
        })
        .application_id(config.discord.application_id.into())
        .await?;

    client.start().await?;
    Ok(())
}
