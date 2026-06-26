use crate::{BunnyClient, CommandReply};
use bunny_i18n::{Locale, t};
use serenity::all::{
    ChannelId, CreateActionRow, CreateButton, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateMessage,
    CreateThread, EditInteractionResponse, EditMessage, Http, Message, MessageId, ReactionType,
    UserId,
};
use serenity::model::application::ButtonStyle;
use serenity::model::channel::ChannelType;
use serenity::model::id::GuildId;
use serenity::prelude::Context;
use std::sync::Arc;
use std::time::Duration;

const THREAD_CLAUDE_TIMEOUT: Duration = Duration::from_secs(300);
/// Discord typing indicator expires after ~10s; refresh while Claude runs.
const THREAD_TYPING_REFRESH: Duration = Duration::from_secs(8);
async fn post_thread_working_message(
    http: &Http,
    channel_id: ChannelId,
    locale: Locale,
) -> Option<MessageId> {
    channel_id
        .send_message(
            http,
            CreateMessage::new().content(t(locale, "discord.thread.working", &[])),
        )
        .await
        .ok()
        .map(|m| m.id)
}

fn spawn_thread_typing_refresh(http: Arc<Http>, channel_id: ChannelId) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let _ = channel_id.broadcast_typing(&http).await;
            tokio::time::sleep(THREAD_TYPING_REFRESH).await;
        }
    })
}

async fn clear_thread_working(
    http: &Http,
    channel_id: ChannelId,
    status_msg_id: Option<MessageId>,
    typing_task: Option<tokio::task::JoinHandle<()>>,
) {
    if let Some(task) = typing_task {
        task.abort();
    }
    if let Some(id) = status_msg_id {
        let _ = channel_id.delete_message(http, id).await;
    }
}

pub struct ThreadRuntime;

impl ThreadRuntime {
    pub fn new() -> Self {
        Self
    }
}

pub fn strip_bot_mention(content: &str, bot_id: UserId) -> String {
    let mention = format!("<@{}>", bot_id.get());
    let mention_nick = format!("<@!{}>", bot_id.get());
    content
        .replace(&mention, "")
        .replace(&mention_nick, "")
        .trim()
        .to_string()
}

pub fn message_mentions_bot(msg: &Message, bot_id: UserId) -> bool {
    msg.mentions.iter().any(|u| u.id == bot_id)
}

pub async fn parent_channel_id_for_id(http: &Http, channel_id: ChannelId) -> String {
    if let Ok(channel) = channel_id.to_channel(http).await {
        if let serenity::model::channel::Channel::Guild(guild_ch) = channel {
            if guild_ch.kind == ChannelType::PublicThread
                || guild_ch.kind == ChannelType::PrivateThread
            {
                if let Some(parent) = guild_ch.parent_id {
                    return parent.get().to_string();
                }
            }
        }
    }
    channel_id.get().to_string()
}

pub async fn parent_channel_id(http: &Http, msg: &Message) -> String {
    if let Ok(channel) = msg.channel_id.to_channel(http).await {
        if let serenity::model::channel::Channel::Guild(guild_ch) = channel {
            if guild_ch.kind == ChannelType::PublicThread
                || guild_ch.kind == ChannelType::PrivateThread
            {
                if let Some(parent) = guild_ch.parent_id {
                    return parent.get().to_string();
                }
            }
        }
    }
    msg.channel_id.get().to_string()
}

pub fn bridge_ctx(
    guild_id: Option<GuildId>,
    parent_channel_id: &str,
    thread_id: Option<&str>,
    discord_user_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "guild_id": guild_id.map(|g| g.get().to_string()).unwrap_or_default(),
        "channel_id": parent_channel_id,
        "thread_id": thread_id.unwrap_or(parent_channel_id),
        "discord_user_id": discord_user_id,
    })
}

pub async fn handle_message(
    ctx: &Context,
    msg: &Message,
    bunny: &BunnyClient,
    bot_id: UserId,
    _runtime: &Arc<ThreadRuntime>,
) -> anyhow::Result<()> {
    if msg.author.bot {
        return Ok(());
    }

    let parent_ch = parent_channel_id(&ctx.http, msg).await;
    let thread_id_str = msg.channel_id.get().to_string();
    let in_thread = parent_ch != thread_id_str;
    let bctx = bridge_ctx(
        msg.guild_id,
        &parent_ch,
        Some(&thread_id_str),
        &msg.author.id.get().to_string(),
    );

    if in_thread {
        return handle_thread_message(ctx, msg, bunny, bot_id, &bctx, &thread_id_str).await;
    }

    if !message_mentions_bot(msg, bot_id) {
        return Ok(());
    }

    let prompt = strip_bot_mention(&msg.content, bot_id);
    if prompt.is_empty() {
        return Ok(());
    }

    let q = query_pairs(&bctx);
    let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let locale = crate::user_locale(bunny, &bctx).await;
    let linked = bunny.get_json("/status", &q_ref).await.is_ok();
    if !linked {
        msg.channel_id
            .say(&ctx.http, t(locale, "discord.thread.not_linked", &[]))
            .await?;
        return Ok(());
    }

    let thread_name = truncate_thread_name(&prompt);
    let thread_ch = msg
        .channel_id
        .create_thread_from_message(
            &ctx.http,
            msg.id,
            CreateThread::new(&thread_name).kind(ChannelType::PublicThread),
        )
        .await?;

    let thread_id = thread_ch.id.get().to_string();
    let mut bind_body = bctx.clone();
    bind_body["thread_id"] = serde_json::json!(thread_id);
    bind_body["goal_text"] = serde_json::json!(prompt);

    let working_id = post_thread_working_message(&ctx.http, thread_ch.id, locale).await;
    let typing_task = spawn_thread_typing_refresh(ctx.http.clone(), thread_ch.id);

    let bind = match bunny
        .post_json_timeout("/thread/bind", &bind_body, THREAD_CLAUDE_TIMEOUT, None)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(%thread_id, "thread bind failed: {e}");
            clear_thread_working(&ctx.http, thread_ch.id, working_id, Some(typing_task)).await;
            thread_ch
                .send_message(
                    &ctx.http,
                    CreateMessage::new().content(t(
                        locale,
                        "discord.thread.bind_failed",
                        &[("error", &e.to_string())],
                    )),
                )
                .await?;
            return Ok(());
        }
    };

    clear_thread_working(&ctx.http, thread_ch.id, working_id, Some(typing_task)).await;

    let extra = if bind.get("git_enabled").and_then(|v| v.as_bool()) == Some(true) {
        t(
            locale,
            "discord.thread.branch_suffix",
            &[(
                "branch",
                bind.get("thread_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
            )],
        )
    } else {
        String::new()
    };
    let goal_msg = thread_ch
        .send_message(
            &ctx.http,
            CreateMessage::new().content(t(
                locale,
                "discord.thread.linked",
                &[
                    ("goal", &prompt),
                    (
                        "shell",
                        bind.get("shell_name").and_then(|v| v.as_str()).unwrap_or("?"),
                    ),
                    (
                        "cwd",
                        bind.get("project_cwd").and_then(|v| v.as_str()).unwrap_or("?"),
                    ),
                    ("extra", &extra),
                ],
            )),
        )
        .await?;

    post_goal_buttons(&ctx.http, thread_ch.id, goal_msg.id, &thread_id).await;

    post_thread_claude_response(
        &ctx.http,
        thread_ch.id,
        &bind,
        goal_msg.id,
        &thread_id,
        &bctx,
        bunny,
        locale,
    )
    .await?;

    upload_message_attachments(bunny, &bind_body, &thread_id, msg).await?;

    Ok(())
}

async fn handle_thread_message(
    ctx: &Context,
    msg: &Message,
    bunny: &BunnyClient,
    bot_id: UserId,
    bctx: &serde_json::Value,
    thread_id: &str,
) -> anyhow::Result<()> {
    let mut body = bctx.clone();
    body["thread_id"] = serde_json::json!(thread_id);

    let status = bunny.post_json("/thread/status", &body, None).await;
    let status = match status {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let st = status.get("status").and_then(|v| v.as_str()).unwrap_or("active");
    let locale = crate::user_locale(bunny, bctx).await;
    if st != "active" {
        msg.channel_id
            .say(&ctx.http, t(locale, "discord.thread.inactive", &[]))
            .await?;
        return Ok(());
    }

    let reply_to_bot = msg
        .referenced_message
        .as_ref()
        .map(|m| m.author.id == bot_id)
        .unwrap_or(false);
    let mentions_bot = message_mentions_bot(msg, bot_id);

    if reply_to_bot || mentions_bot {
        let text = strip_bot_mention(&msg.content, bot_id);
        if text.is_empty() && msg.attachments.is_empty() {
            return Ok(());
        }
        upload_message_attachments(bunny, &body, thread_id, msg).await?;
        let mut prompt = text;
        if !msg.attachments.is_empty() {
            prompt.push_str("\n\n_(pièces jointes enregistrées sur le serveur — voir chemins dans les logs thread)_");
        }
        let input_body = serde_json::json!({
            "guild_id": bctx["guild_id"],
            "channel_id": bctx["channel_id"],
            "thread_id": thread_id,
            "discord_user_id": bctx["discord_user_id"],
            "text": prompt,
            "discord_message_id": msg.id.get().to_string(),
            "author_name": msg.author.name,
            "include_discussion_context": true,
        });

        let working_id = post_thread_working_message(&ctx.http, msg.channel_id, locale).await;
        let typing_task = spawn_thread_typing_refresh(ctx.http.clone(), msg.channel_id);

        let res = match bunny
            .post_json_timeout("/thread/input", &input_body, THREAD_CLAUDE_TIMEOUT, None)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(%thread_id, "thread input failed: {e}");
                clear_thread_working(
                    &ctx.http,
                    msg.channel_id,
                    working_id,
                    Some(typing_task),
                )
                .await;
                msg.channel_id
                    .send_message(
                        &ctx.http,
                        CreateMessage::new().content(format!("❌ Erreur Claude : {e}")),
                    )
                    .await?;
                return Ok(());
            }
        };

        clear_thread_working(&ctx.http, msg.channel_id, working_id, Some(typing_task)).await;

        post_thread_claude_response(
            &ctx.http,
            msg.channel_id,
            &res,
            msg.id,
            thread_id,
            bctx,
            bunny,
            locale,
        )
        .await?;
        return Ok(());
    }

    let discussion_body = serde_json::json!({
        "guild_id": bctx["guild_id"],
        "channel_id": bctx["channel_id"],
        "thread_id": thread_id,
        "discord_user_id": bctx["discord_user_id"],
        "content": msg.content,
        "author_name": msg.author.name,
    });
    bunny.post_json("/thread/discussion", &discussion_body, None).await?;

    Ok(())
}

async fn post_thread_claude_response(
    http: &Http,
    channel_id: ChannelId,
    res: &serde_json::Value,
    input_message_id: MessageId,
    thread_id: &str,
    bctx: &serde_json::Value,
    bunny: &BunnyClient,
    locale: Locale,
) -> anyhow::Result<()> {
    if res.get("needs_approval").and_then(|v| v.as_bool()) == Some(true) {
        let summary = res
            .get("approval_summary")
            .and_then(|v| v.as_str())
            .unwrap_or("commande nécessitant approbation");
        channel_id
            .send_message(
                http,
                CreateMessage::new().content(format!(
                    "⚠️ Cette action nécessite une approbation :\n```\n{summary}\n```"
                )),
            )
            .await?;
        return Ok(());
    }

    if let (Some(pending_id), Some(command)) = (
        res.get("pending_permission_id").and_then(|v| v.as_str()),
        res.get("permission_command").and_then(|v| v.as_str()),
    ) {
        let intro = res
            .get("response_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        post_thread_permission_prompt(http, channel_id, intro, pending_id, command).await?;
        return Ok(());
    }

    let text = res
        .get("response_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if let (Some(pending_id), Some(questions)) = (
        res.get("pending_question_id").and_then(|v| v.as_str()),
        res.get("pending_questions").and_then(|v| v.as_array()),
    ) {
        let intro_owned = if text.trim().is_empty() {
            t(locale, "discord.thread.question_intro", &[])
        } else {
            text.trim().to_string()
        };
        post_thread_question_prompt(
            http,
            channel_id,
            &intro_owned,
            pending_id,
            questions,
            thread_id,
            0,
            true,
        )
        .await?;
        return Ok(());
    }

    if text.trim().is_empty() {
        channel_id
            .send_message(
                http,
                CreateMessage::new().content("_(Claude n’a renvoyé aucune réponse textuelle.)_"),
            )
            .await?;
        return Ok(());
    }

    for page in crate::paginate_plain(text) {
        channel_id
            .send_message(http, CreateMessage::new().content(page))
            .await?;
    }

    let _ = (input_message_id, bctx, bunny, thread_id);
    Ok(())
}

/// Discord button custom_id: `bunny:tqa:{pending_id}:{q_index}:{opt_index}` (max 100 chars).
pub fn parse_thread_question_button(custom_id: &str) -> Option<(String, usize, usize)> {
    let rest = custom_id.strip_prefix("bunny:tqa:")?;
    let mut parts = rest.splitn(3, ':');
    let pending_id = parts.next()?.to_string();
    let q_index: usize = parts.next()?.parse().ok()?;
    let opt_index: usize = parts.next()?.parse().ok()?;
    Some((pending_id, q_index, opt_index))
}

pub fn thread_question_button_id(pending_id: &str, q_index: usize, opt_index: usize) -> String {
    format!("bunny:tqa:{pending_id}:{q_index}:{opt_index}")
}

/// Discord button custom_id: `bunny:tperm:{pending_id}:{1|0}` (approve/deny).
pub fn parse_thread_permission_button(custom_id: &str) -> Option<(String, bool)> {
    let rest = custom_id.strip_prefix("bunny:tperm:")?;
    let mut parts = rest.splitn(2, ':');
    let pending_id = parts.next()?.to_string();
    let approve = parts.next()? == "1";
    Some((pending_id, approve))
}

pub fn thread_permission_button_id(pending_id: &str, approve: bool) -> String {
    format!("bunny:tperm:{pending_id}:{}", if approve { 1 } else { 0 })
}

pub fn thread_merge_button_id(thread_id: &str) -> String {
    format!("bunny:tmerge:{thread_id}")
}

pub fn parse_thread_merge_button(custom_id: &str) -> Option<String> {
    custom_id.strip_prefix("bunny:tmerge:").map(str::to_string)
}

pub async fn handle_thread_merge_button(
    comp: &serenity::model::application::ComponentInteraction,
    http: &Http,
    bunny: &BunnyClient,
) -> anyhow::Result<()> {
    let thread_id = parse_thread_merge_button(&comp.data.custom_id)
        .ok_or_else(|| anyhow::anyhow!("invalid merge button"))?;

    let parent_ch = parent_channel_id_for_id(http, comp.channel_id).await;
    comp.create_response(
        http,
        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
    )
    .await?;

    let bctx = bridge_ctx(
        comp.guild_id,
        &parent_ch,
        Some(&thread_id),
        &comp.user.id.get().to_string(),
    );
    let locale = crate::user_locale(bunny, &bctx).await;
    let mut body = bctx;
    body["thread_id"] = serde_json::json!(thread_id);

    let res = bunny.post_json("/thread/merge", &body, None).await?;
    let base = res
        .get("base_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main");
    let mut content = t(
        locale,
        "discord.thread.merge_success",
        &[("base", base)],
    );
    if let Some(output) = res.get("output").and_then(|v| v.as_str()) {
        if !output.trim().is_empty() {
            content.push_str("\n```\n");
            let out = if output.len() > 1500 {
                format!("{}…", &output[..1500])
            } else {
                output.to_string()
            };
            content.push_str(&out);
            content.push_str("\n```");
        }
    }

    comp.create_followup(http, CreateInteractionResponseFollowup::new().content(&content))
        .await?;

    let _ = comp
        .channel_id
        .edit_message(
            http,
            comp.message.id,
            EditMessage::new().components(vec![]),
        )
        .await;

    Ok(())
}

fn truncate_shell_command_for_discord(command: &str) -> String {
    const MAX: usize = 600;
    let cmd = command.trim();
    let n = cmd.chars().count();
    if n <= MAX {
        return cmd.to_string();
    }
    let truncated: String = cmd.chars().take(MAX).collect();
    format!("{truncated}\n… ({n} chars)")
}

async fn post_thread_permission_prompt(
    http: &Http,
    channel_id: ChannelId,
    intro: &str,
    pending_id: &str,
    command: &str,
) -> anyhow::Result<()> {
    let mut content = String::new();
    if !intro.trim().is_empty() {
        content.push_str(intro.trim());
        content.push_str("\n\n");
    }
    content.push_str("**Commande :**\n```bash\n");
    content.push_str(&truncate_shell_command_for_discord(command));
    content.push_str("\n```");
    if content.len() > 1900 {
        content.truncate(1900);
        content.push_str("…");
    }

    channel_id
        .send_message(
            http,
            CreateMessage::new().content(content).components(vec![
                CreateActionRow::Buttons(vec![
                    CreateButton::new(thread_permission_button_id(pending_id, true))
                        .label("Autoriser")
                        .style(ButtonStyle::Success),
                    CreateButton::new(thread_permission_button_id(pending_id, false))
                        .label("Refuser")
                        .style(ButtonStyle::Danger),
                ]),
            ]),
        )
        .await?;
    Ok(())
}

pub async fn handle_thread_permission_button(
    comp: &serenity::model::application::ComponentInteraction,
    http: Arc<Http>,
    bunny: &BunnyClient,
) -> anyhow::Result<()> {
    let (pending_id, approve) = parse_thread_permission_button(&comp.data.custom_id)
        .ok_or_else(|| anyhow::anyhow!("invalid permission button"))?;

    let parent_ch = parent_channel_id_for_id(&http, comp.channel_id).await;
    let thread_id = comp.channel_id.get().to_string();

    comp.create_response(
        &http,
        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
    )
    .await?;

    let bctx = bridge_ctx(
        comp.guild_id,
        &parent_ch,
        Some(&thread_id),
        &comp.user.id.get().to_string(),
    );
    let locale = crate::user_locale(bunny, &bctx).await;
    let mut body = bctx.clone();
    body["thread_id"] = serde_json::json!(thread_id);
    body["pending_id"] = serde_json::json!(pending_id);
    body["approve"] = serde_json::json!(approve);

    let _ = comp
        .edit_response(
            &http,
            EditInteractionResponse::new()
                .content(if approve {
                    "✓ Permission accordée — Claude reprend…"
                } else {
                    "✗ Permission refusée — Claude reprend…"
                })
                .components(vec![]),
        )
        .await;

    let working_id = post_thread_working_message(&http, comp.channel_id, locale).await;
    let typing_task = spawn_thread_typing_refresh(http.clone(), comp.channel_id);

    let res = match bunny
        .post_json_timeout("/thread/permission", &body, THREAD_CLAUDE_TIMEOUT, None)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            clear_thread_working(&http, comp.channel_id, working_id, Some(typing_task)).await;
            comp.create_followup(
                &http,
                CreateInteractionResponseFollowup::new().content(format!("❌ Erreur : {e}")),
            )
            .await?;
            return Ok(());
        }
    };

    clear_thread_working(&http, comp.channel_id, working_id, Some(typing_task)).await;

    post_thread_claude_response(
        &http,
        comp.channel_id,
        &res,
        comp.message.id,
        &thread_id,
        &bctx,
        bunny,
        locale,
    )
    .await
}

async fn post_thread_question_prompt(
    http: &Http,
    channel_id: ChannelId,
    intro: &str,
    pending_id: &str,
    questions: &[serde_json::Value],
    thread_id: &str,
    question_index: usize,
    include_intro: bool,
) -> anyhow::Result<()> {
    let Some(q) = questions.get(question_index) else {
        return Ok(());
    };
    let question_text = q
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or("Question");
    let header = q
        .get("header")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Choix");
    let multi = q
        .get("multiSelect")
        .or_else(|| q.get("multi_select"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let options = q
        .get("options")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let total = questions.len();
    let mut content = String::new();
    if include_intro && !intro.trim().is_empty() {
        content.push_str(intro.trim());
        content.push_str("\n\n");
    }
    content.push_str(&format!(
        "**{}/{} · {header}**\n{question_text}",
        question_index + 1,
        total
    ));
    if multi {
        content.push_str(
            "\n\n_(sélection multiple : une option par clic pour l’instant)_",
        );
    }
    if content.len() > 1900 {
        content.truncate(1900);
        content.push_str("…");
    }

    let mut buttons = Vec::new();
    for (opt_index, opt) in options.iter().take(5).enumerate() {
        let label = opt
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let btn_label = if label.chars().count() > 80 {
            format!("{}…", label.chars().take(77).collect::<String>())
        } else {
            label.to_string()
        };
        buttons.push(
            CreateButton::new(thread_question_button_id(
                pending_id,
                question_index,
                opt_index,
            ))
            .label(btn_label)
            .style(ButtonStyle::Primary),
        );
    }

    let components = if buttons.is_empty() {
        vec![]
    } else {
        vec![CreateActionRow::Buttons(buttons)]
    };

    channel_id
        .send_message(
            http,
            CreateMessage::new()
                .content(content)
                .components(components),
        )
        .await?;

    let _ = thread_id;
    Ok(())
}

pub async fn handle_thread_question_button(
    comp: &serenity::model::application::ComponentInteraction,
    http: Arc<Http>,
    bunny: &BunnyClient,
) -> anyhow::Result<()> {
    let (pending_id, q_index, opt_index) =
        parse_thread_question_button(&comp.data.custom_id)
            .ok_or_else(|| anyhow::anyhow!("invalid question button"))?;

    let parent_ch = parent_channel_id_for_id(&http, comp.channel_id).await;
    let thread_id = comp.channel_id.get().to_string();

    comp.create_response(
        &http,
        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
    )
    .await?;

    let answer_label = answer_label_from_message_components(comp, opt_index);

    let bctx = bridge_ctx(
        comp.guild_id,
        &parent_ch,
        Some(&thread_id),
        &comp.user.id.get().to_string(),
    );
    let locale = crate::user_locale(bunny, &bctx).await;
    let mut body = bctx.clone();
    body["pending_id"] = serde_json::json!(pending_id);
    body["question_index"] = serde_json::json!(q_index);
    body["answer_label"] = serde_json::json!(answer_label);

    let _ = comp
        .edit_response(
            &http,
            EditInteractionResponse::new()
                .content("✓ Choix enregistré — Claude reprend…")
                .components(vec![]),
        )
        .await;

    let working_id = post_thread_working_message(&http, comp.channel_id, locale).await;
    let typing_task = spawn_thread_typing_refresh(http.clone(), comp.channel_id);

    let res = match bunny
        .post_json_timeout("/thread/answer", &body, THREAD_CLAUDE_TIMEOUT, None)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            clear_thread_working(&http, comp.channel_id, working_id, Some(typing_task)).await;
            comp.create_followup(
                &http,
                CreateInteractionResponseFollowup::new().content(format!("❌ Erreur : {e}")),
            )
            .await?;
            return Ok(());
        }
    };

    clear_thread_working(&http, comp.channel_id, working_id, Some(typing_task)).await;

    if res.get("complete").and_then(|v| v.as_bool()) != Some(true) {
        let next = res
            .get("next_question_index")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        if let Some(questions) = res.get("pending_questions").and_then(|v| v.as_array()) {
            post_thread_question_prompt(
                &http,
                comp.channel_id,
                "",
                &pending_id,
                questions,
                &thread_id,
                next,
                false,
            )
            .await?;
        }
        return Ok(());
    }

    let claude_payload = res.get("claude").cloned().unwrap_or(res.clone());
    post_thread_claude_response(
        &http,
        comp.channel_id,
        &claude_payload,
        comp.message.id,
        &thread_id,
        &bctx,
        bunny,
        locale,
    )
    .await?;

    Ok(())
}

fn answer_label_from_message_components(
    comp: &serenity::model::application::ComponentInteraction,
    opt_index: usize,
) -> String {
    comp.message
        .components
        .first()
        .and_then(|row| {
            if let serenity::all::ActionRowComponent::Button(b) = row.components.get(opt_index)? {
                b.label.clone()
            } else {
                None
            }
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("option-{opt_index}"))
}

fn query_pairs(ctx: &serde_json::Value) -> Vec<(String, String)> {
    vec![
        ("guild_id".into(), ctx["guild_id"].as_str().unwrap_or("").into()),
        ("channel_id".into(), ctx["channel_id"].as_str().unwrap_or("").into()),
        (
            "discord_user_id".into(),
            ctx["discord_user_id"].as_str().unwrap_or("").into(),
        ),
    ]
}

fn truncate_thread_name(prompt: &str) -> String {
    let one_line: String = prompt.lines().next().unwrap_or(prompt).chars().take(80).collect();
    if one_line.is_empty() {
        "bunny-task".into()
    } else {
        one_line
    }
}

pub async fn post_goal_buttons(
    http: &Http,
    channel_id: ChannelId,
    message_id: MessageId,
    thread_id: &str,
) {
    let _ = channel_id
        .edit_message(
            http,
            message_id,
            EditMessage::new().components(vec![CreateActionRow::Buttons(vec![
                CreateButton::new(format!("bunny:goal:{thread_id}"))
                    .label("Goal !")
                    .style(ButtonStyle::Success),
                CreateButton::new(format!("bunny:cancel:{thread_id}"))
                    .label("Cancel")
                    .style(ButtonStyle::Danger),
            ])]),
        )
        .await;
}

pub async fn handle_goal_cancel_button(
    comp: &serenity::model::application::ComponentInteraction,
    http: &Http,
    bunny: &BunnyClient,
    approve: bool,
) -> anyhow::Result<()> {
    let parent_ch = parent_channel_id_for_id(http, comp.channel_id).await;
    let thread_id = comp.channel_id.get().to_string();
    let bctx = bridge_ctx(
        comp.guild_id,
        &parent_ch,
        Some(&thread_id),
        &comp.user.id.get().to_string(),
    );
    let locale = crate::user_locale(bunny, &bctx).await;
    let mut body = bctx;
    body["thread_id"] = serde_json::json!(thread_id);
    body["outcome"] = serde_json::json!(if approve { "goal" } else { "cancel" });

    comp.create_response(
        http,
        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
    )
    .await?;

    let res = bunny.post_json("/thread/finalize", &body, None).await?;
    let status_emoji = if approve { "✅" } else { "❌" };
    let offer_merge = res.get("offer_merge").and_then(|v| v.as_bool()) == Some(true);
    let merge_base = res
        .get("merge_base_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main");
    let merge_thread = res
        .get("merge_thread_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("bunny/thread");
    let mut content: String = if approve {
        t(locale, "discord.thread.goal_confirmed", &[])
    } else {
        t(locale, "discord.thread.goal_cancelled", &[])
    };
    if offer_merge {
        content.push_str("\n\n");
        content.push_str(&t(
            locale,
            "discord.thread.goal_git_instructions",
            &[("branch", merge_thread), ("base", merge_base)],
        ));
    }

    let mut followup = CreateInteractionResponseFollowup::new().content(&content);
    if offer_merge {
        let thread_id = comp.channel_id.get().to_string();
        let mut btn_label = t(
            locale,
            "discord.thread.merge_button",
            &[("base", merge_base)],
        );
        if btn_label.chars().count() > 80 {
            btn_label = btn_label.chars().take(77).collect::<String>() + "…";
        }
        followup = followup.components(vec![CreateActionRow::Buttons(vec![
            CreateButton::new(thread_merge_button_id(&thread_id))
                .label(btn_label)
                .style(ButtonStyle::Primary),
        ])]);
    }

    comp.create_followup(http, followup).await?;

    let _ = comp
        .channel_id
        .edit_message(
            http,
            comp.message.id,
            EditMessage::new().components(vec![]),
        )
        .await;

    rename_thread_with_status(http, comp.channel_id, status_emoji).await;

    Ok(())
}

async fn rename_thread_with_status(http: &Http, channel_id: ChannelId, status_emoji: &str) {
    let base_name = channel_id
        .to_channel(http)
        .await
        .ok()
        .and_then(|ch| {
            if let serenity::model::channel::Channel::Guild(g) = ch {
                Some(g.name)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "thread".to_string());
    let stripped = strip_thread_status_prefix(&base_name);
    let mut new_name = format!("{status_emoji} {stripped}");
    if new_name.chars().count() > 100 {
        new_name = new_name.chars().take(100).collect();
    }
    let _ = channel_id
        .edit(http, serenity::builder::EditChannel::new().name(new_name))
        .await;
}

fn strip_thread_status_prefix(name: &str) -> &str {
    let trimmed = name.trim();
    for prefix in ["✅ ", "❌ ", "✅", "❌"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }
    trimmed
}

pub async fn handle_stop_reaction(
    ctx: &Context,
    bunny: &BunnyClient,
    thread_id: &str,
    bctx: &serde_json::Value,
    message_id: u64,
) -> anyhow::Result<()> {
    let mut body = bctx.clone();
    body["thread_id"] = serde_json::json!(thread_id);

    let status = bunny.post_json("/thread/status", &body, None).await?;
    if status.get("status").and_then(|v| v.as_str()) != Some("active") {
        return Ok(());
    }

    let last_input = status
        .get("last_input_discord_message_id")
        .and_then(|v| v.as_str());
    if last_input != Some(&message_id.to_string()) {
        return Ok(());
    }

    bunny.post_json("/thread/stop", &body, None).await?;

    let channel_id = ChannelId::new(thread_id.parse()?);
    channel_id
        .send_message(&ctx.http, CreateMessage::new().content("⛔ Interrompu."))
        .await?;

    Ok(())
}

pub fn is_stop_emoji(reaction: &ReactionType) -> bool {
    match reaction {
        ReactionType::Unicode(s) => s == "⛔" || s == "🛑" || s == "🚫",
        _ => false,
    }
}

async fn upload_message_attachments(
    bunny: &BunnyClient,
    bctx: &serde_json::Value,
    thread_id: &str,
    msg: &Message,
) -> anyhow::Result<()> {
    for att in &msg.attachments {
        let bytes = reqwest::get(&att.url).await?.bytes().await?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let mut body = bctx.clone();
        body["thread_id"] = serde_json::json!(thread_id);
        body["filename"] = serde_json::json!(att.filename);
        body["content_base64"] = serde_json::json!(b64);
        let _ = bunny.post_json("/thread/attachment", &body, None).await;
    }
    Ok(())
}

pub async fn run_git_command(
    bunny: &BunnyClient,
    bctx: &serde_json::Value,
    sub: &str,
    branch: Option<&str>,
    path: Option<&str>,
    _locale: Locale,
) -> Result<CommandReply, anyhow::Error> {
    let body = serde_json::json!({
        "guild_id": bctx["guild_id"],
        "channel_id": bctx["channel_id"],
        "discord_user_id": bctx["discord_user_id"],
        "subcommand": sub,
        "branch": branch,
        "path": path,
    });
    let res = bunny.post_json("/git", &body, None).await?;
    if let Some(output) = res.get("output").and_then(|v| v.as_str()) {
        Ok(CommandReply::Text(crate::paginate_plain(output)))
    } else {
        Ok(CommandReply::Text(vec![format!(
            "```json\n{}\n```",
            serde_json::to_string_pretty(&res)?
        )]))
    }
}

pub async fn run_project_command(
    bunny: &BunnyClient,
    bctx: &serde_json::Value,
    path: Option<&str>,
    _locale: Locale,
) -> Result<CommandReply, anyhow::Error> {
    if let Some(p) = path {
        let body = serde_json::json!({
            "guild_id": bctx["guild_id"],
            "channel_id": bctx["channel_id"],
            "discord_user_id": bctx["discord_user_id"],
            "path": p,
        });
        let res = bunny.post_json("/project/set", &body, None).await?;
        Ok(CommandReply::Text(vec![format!(
            "Project cwd: `{}` (git: {})",
            res.get("project_cwd").and_then(|v| v.as_str()).unwrap_or("?"),
            res.get("git_enabled").and_then(|v| v.as_bool()).unwrap_or(false)
        )]))
    } else {
        let q = query_pairs(bctx);
        let q_ref: Vec<(&str, String)> = q.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let res = bunny.get_json("/project", &q_ref).await?;
        Ok(CommandReply::Text(vec![format!(
            "**Project**\n- cwd: `{}`\n- git: {}",
            res.get("project_cwd").and_then(|v| v.as_str()).unwrap_or("?"),
            res.get("git_enabled").and_then(|v| v.as_bool()).unwrap_or(false)
        )]))
    }
}
