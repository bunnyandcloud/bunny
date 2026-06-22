use crate::models::{
    AgentTask, AgentTaskMode, AgentTaskStatus, ApprovalRequest, AskUserQuestionItem,
    DiscordAuditEntry, DiscordFollow, DiscordSessionLink, DiscordUserLink,
    DiscordThreadBinding, DiscordThreadDiscussion, DiscordThreadMessage, DiscordThreadMessageRole,
    DiscordThreadPendingPermission, DiscordThreadPendingQuestions, DiscordThreadStatus,
    WatchSession,
};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub struct DiscordDb {
    conn: Connection,
}

impl DiscordDb {
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS discord_installations (
                guild_id TEXT PRIMARY KEY,
                installed_by_user_id TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS discord_session_links (
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                created_by_user_id TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL,
                PRIMARY KEY (guild_id, channel_id)
            );
            CREATE TABLE IF NOT EXISTS discord_link_codes (
                code TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                created_by_user_id TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                used_at TEXT
            );
            CREATE TABLE IF NOT EXISTS discord_thread_bindings (
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                default_shell_id TEXT,
                created_at TEXT NOT NULL,
                PRIMARY KEY (thread_id)
            );
            CREATE TABLE IF NOT EXISTS agent_tasks (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL,
                discord_thread_id TEXT,
                requested_by_discord_id TEXT,
                requested_by_user_id TEXT,
                agent TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                prompt TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS approval_requests (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                action_summary TEXT NOT NULL,
                reason TEXT NOT NULL,
                status TEXT NOT NULL,
                discord_message_id TEXT,
                created_at TEXT NOT NULL,
                resolved_at TEXT
            );
            CREATE TABLE IF NOT EXISTS watch_sessions (
                id TEXT PRIMARY KEY,
                token TEXT NOT NULL UNIQUE,
                session_id TEXT NOT NULL,
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                thread_id TEXT,
                layout TEXT NOT NULL,
                visibility TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                required_role_ids TEXT NOT NULL DEFAULT '[]',
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS discord_user_links (
                discord_user_id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS discord_audit_log (
                id TEXT PRIMARY KEY,
                discord_user_id TEXT,
                bunny_user_id TEXT,
                guild_id TEXT,
                channel_id TEXT,
                thread_id TEXT,
                session_id TEXT,
                command TEXT NOT NULL,
                action_executed TEXT NOT NULL,
                agent TEXT,
                shell_id TEXT,
                browser_id TEXT,
                approval_id TEXT,
                result TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS discord_follows (
                id TEXT PRIMARY KEY,
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                target TEXT NOT NULL,
                shell_name TEXT,
                interval_secs INTEGER NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_discord_links_session ON discord_session_links(session_id);
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_session ON agent_tasks(session_id);
            CREATE INDEX IF NOT EXISTS idx_watch_token ON watch_sessions(token);
            "#,
        )?;
        let _ = self.conn.execute(
            "ALTER TABLE watch_sessions ADD COLUMN browser_id TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE discord_session_links ADD COLUMN last_shell_name TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE discord_session_links ADD COLUMN claude_session_id TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE discord_session_links ADD COLUMN project_cwd_override TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE discord_thread_bindings ADD COLUMN term_id TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE discord_user_links ADD COLUMN discord_username TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE discord_user_links ADD COLUMN discord_global_name TEXT",
            [],
        );
        let _ = self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discord_user_links_user ON discord_user_links(user_id)",
            [],
        );
        for col in [
            "ALTER TABLE discord_thread_bindings ADD COLUMN project_cwd TEXT",
            "ALTER TABLE discord_thread_bindings ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
            "ALTER TABLE discord_thread_bindings ADD COLUMN goal_text TEXT",
            "ALTER TABLE discord_thread_bindings ADD COLUMN git_enabled INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE discord_thread_bindings ADD COLUMN base_branch TEXT",
            "ALTER TABLE discord_thread_bindings ADD COLUMN thread_branch TEXT",
            "ALTER TABLE discord_thread_bindings ADD COLUMN start_commit TEXT",
            "ALTER TABLE discord_thread_bindings ADD COLUMN last_pane_marker INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE discord_thread_bindings ADD COLUMN last_input_discord_message_id TEXT",
            "ALTER TABLE discord_thread_bindings ADD COLUMN last_pane_snapshot TEXT NOT NULL DEFAULT ''",
        ] {
            let _ = self.conn.execute(col, []);
        }
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS discord_thread_discussion (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                discord_user_id TEXT NOT NULL,
                author_name TEXT,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread_discussion ON discord_thread_discussion(thread_id);
            CREATE TABLE IF NOT EXISTS discord_thread_messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                discord_user_id TEXT,
                author_name TEXT,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread_messages ON discord_thread_messages(thread_id, created_at);
            CREATE TABLE IF NOT EXISTS discord_thread_pending_questions (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                questions_json TEXT NOT NULL,
                answers_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread_pending ON discord_thread_pending_questions(thread_id);
            CREATE TABLE IF NOT EXISTS discord_thread_pending_permissions (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                claude_session_id TEXT,
                command TEXT NOT NULL,
                allowed_tools_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread_pending_perm ON discord_thread_pending_permissions(thread_id);
            CREATE TABLE IF NOT EXISTS discord_thread_denied_shell_commands (
                thread_id TEXT NOT NULL,
                command_key TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (thread_id, command_key)
            );
            CREATE TABLE IF NOT EXISTS discord_thread_granted_shell_commands (
                thread_id TEXT NOT NULL,
                command_key TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (thread_id, command_key)
            );
            "#,
        )?;
        let _ = self.conn.execute(
            "ALTER TABLE discord_thread_bindings ADD COLUMN claude_session_id TEXT",
            [],
        );
        Ok(())
    }

    pub fn generate_link_code(&self, session_id: Uuid, user_id: Uuid, ttl_minutes: i64) -> Result<String> {
        let code: String = {
            const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
            let mut rng = rand::thread_rng();
            (0..8)
                .map(|_| {
                    let idx = rng.gen_range(0..CHARSET.len());
                    CHARSET[idx] as char
                })
                .collect()
        };
        let expires = Utc::now() + Duration::minutes(ttl_minutes);
        self.conn.execute(
            "INSERT INTO discord_link_codes (code, session_id, created_by_user_id, expires_at) VALUES (?1,?2,?3,?4)",
            params![code, session_id.to_string(), user_id.to_string(), expires.to_rfc3339()],
        )?;
        Ok(code)
    }

    pub fn consume_link_code(&self, code: &str) -> Result<(Uuid, Uuid)> {
        let now = Utc::now().to_rfc3339();
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT session_id, created_by_user_id FROM discord_link_codes WHERE code = ?1 AND used_at IS NULL AND expires_at > ?2",
                params![code.to_uppercase(), now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (session_id, user_id) = row.ok_or_else(|| anyhow::anyhow!("invalid or expired link code"))?;
        self.conn.execute(
            "UPDATE discord_link_codes SET used_at = ?1 WHERE code = ?2",
            params![Utc::now().to_rfc3339(), code.to_uppercase()],
        )?;
        Ok((
            Uuid::parse_str(&session_id)?,
            Uuid::parse_str(&user_id)?,
        ))
    }

    pub fn upsert_session_link(
        &self,
        guild_id: &str,
        channel_id: &str,
        session_id: Uuid,
        created_by: Option<Uuid>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"INSERT INTO discord_session_links (guild_id, channel_id, session_id, created_by_user_id, status, created_at)
               VALUES (?1,?2,?3,?4,'active',?5)
               ON CONFLICT(guild_id, channel_id) DO UPDATE SET session_id=excluded.session_id, status='active'"#,
            params![
                guild_id,
                channel_id,
                session_id.to_string(),
                created_by.map(|u| u.to_string()),
                now
            ],
        )?;
        Ok(())
    }

    pub fn remove_session_link(&self, guild_id: &str, channel_id: &str) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM discord_session_links WHERE guild_id = ?1 AND channel_id = ?2",
            params![guild_id, channel_id],
        )?;
        Ok(n > 0)
    }

    pub fn get_last_shell_name(&self, guild_id: &str, channel_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT last_shell_name FROM discord_session_links WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active'",
                params![guild_id, channel_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(Into::into)
            .map(|opt| opt.flatten().filter(|s| !s.is_empty()))
    }

    pub fn set_last_shell_name(&self, guild_id: &str, channel_id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_session_links SET last_shell_name = ?1 WHERE guild_id = ?2 AND channel_id = ?3",
            params![name, guild_id, channel_id],
        )?;
        Ok(())
    }

    /// Active Discord thread bindings for a session (term_id per thread).
    pub fn list_thread_bound_term_ids_for_session(&self, session_id: Uuid) -> Result<Vec<Uuid>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(term_id, default_shell_id) FROM discord_thread_bindings
             WHERE session_id = ?1 AND status = 'active'
             AND COALESCE(term_id, default_shell_id) IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |r| {
            let raw: String = r.get(0)?;
            Uuid::parse_str(&raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn is_term_bound_to_active_thread(&self, term_id: Uuid) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT 1 FROM discord_thread_bindings
                 WHERE status = 'active' AND COALESCE(term_id, default_shell_id) = ?1 LIMIT 1",
                params![term_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map(|opt| opt.is_some())
            .map_err(Into::into)
    }

    pub fn get_claude_session_id(
        &self,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT claude_session_id FROM discord_session_links WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active'",
                params![guild_id, channel_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(Into::into)
            .map(|opt| opt.flatten().filter(|s| !s.is_empty()))
    }

    pub fn set_claude_session_id(
        &self,
        guild_id: &str,
        channel_id: &str,
        session_id: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_session_links SET claude_session_id = ?1 WHERE guild_id = ?2 AND channel_id = ?3",
            params![session_id, guild_id, channel_id],
        )?;
        Ok(())
    }

    pub fn set_project_cwd_override(
        &self,
        guild_id: &str,
        channel_id: &str,
        path: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_session_links SET project_cwd_override = ?1 WHERE guild_id = ?2 AND channel_id = ?3",
            params![path, guild_id, channel_id],
        )?;
        Ok(())
    }

    pub fn get_session_link(&self, guild_id: &str, channel_id: &str) -> Result<Option<DiscordSessionLink>> {
        self.conn
            .query_row(
                "SELECT guild_id, channel_id, session_id, created_by_user_id, status, project_cwd_override, created_at FROM discord_session_links WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active'",
                params![guild_id, channel_id],
                map_session_link_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_link_status_for_session(&self, session_id: Uuid) -> Result<Vec<DiscordSessionLink>> {
        let mut stmt = self.conn.prepare(
            "SELECT guild_id, channel_id, session_id, created_by_user_id, status, project_cwd_override, created_at FROM discord_session_links WHERE session_id = ?1",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], map_session_link_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_installation(&self, guild_id: &str, installed_by: Option<&str>) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_installations (guild_id, installed_by_user_id, created_at)
               VALUES (?1,?2,?3) ON CONFLICT(guild_id) DO NOTHING"#,
            params![guild_id, installed_by, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn link_discord_user(&self, discord_user_id: &str, user_id: Uuid) -> Result<()> {
        self.link_discord_user_profile(discord_user_id, user_id, None, None)
    }

    pub fn link_discord_user_profile(
        &self,
        discord_user_id: &str,
        user_id: Uuid,
        discord_username: Option<&str>,
        discord_global_name: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_user_links (discord_user_id, user_id, discord_username, discord_global_name, created_at)
               VALUES (?1,?2,?3,?4,?5)
               ON CONFLICT(discord_user_id) DO UPDATE SET
                 user_id=excluded.user_id,
                 discord_username=COALESCE(excluded.discord_username, discord_user_links.discord_username),
                 discord_global_name=COALESCE(excluded.discord_global_name, discord_user_links.discord_global_name)"#,
            params![
                discord_user_id,
                user_id.to_string(),
                discord_username,
                discord_global_name,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_discord_link_for_user(&self, user_id: Uuid) -> Result<Option<DiscordUserLink>> {
        self.conn
            .query_row(
                "SELECT discord_user_id, user_id, discord_username, discord_global_name, created_at
                 FROM discord_user_links WHERE user_id = ?1 LIMIT 1",
                params![user_id.to_string()],
                map_discord_user_link_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn unlink_discord_user(&self, user_id: Uuid) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM discord_user_links WHERE user_id = ?1",
            params![user_id.to_string()],
        )?;
        Ok(n > 0)
    }

    pub fn get_bunny_user_for_discord(&self, discord_user_id: &str) -> Result<Option<Uuid>> {
        self.conn
            .query_row(
                "SELECT user_id FROM discord_user_links WHERE discord_user_id = ?1",
                params![discord_user_id],
                |r| Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Links a Discord user to the Bunny account that created the link code, when that user
    /// previously ran `/bunny link` on this channel but user linking was not persisted (legacy).
    pub fn backfill_discord_user_link(
        &self,
        guild_id: &str,
        channel_id: &str,
        discord_user_id: &str,
    ) -> Result<Option<Uuid>> {
        let ran_link = self
            .conn
            .query_row(
                "SELECT 1 FROM discord_audit_log WHERE guild_id = ?1 AND channel_id = ?2 AND discord_user_id = ?3 AND command = '/bunny link' AND result = 'ok' LIMIT 1",
                params![guild_id, channel_id, discord_user_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !ran_link {
            return Ok(None);
        }

        let session_id: String = self.conn.query_row(
            "SELECT session_id FROM discord_session_links WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active'",
            params![guild_id, channel_id],
            |r| r.get(0),
        )?;

        let creator: Option<String> = self
            .conn
            .query_row(
                "SELECT created_by_user_id FROM discord_link_codes WHERE session_id = ?1 AND used_at IS NOT NULL ORDER BY used_at DESC LIMIT 1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;

        let Some(creator) = creator else {
            return Ok(None);
        };
        let user_id = Uuid::parse_str(&creator)?;
        self.link_discord_user(discord_user_id, user_id)?;
        Ok(Some(user_id))
    }

    pub fn insert_audit(&self, entry: &DiscordAuditEntry) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_audit_log (id, discord_user_id, bunny_user_id, guild_id, channel_id, thread_id,
               session_id, command, action_executed, agent, shell_id, browser_id, approval_id, result, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"#,
            params![
                entry.id.to_string(),
                entry.discord_user_id,
                entry.bunny_user_id.map(|u| u.to_string()),
                entry.guild_id,
                entry.channel_id,
                entry.thread_id,
                entry.session_id.map(|u| u.to_string()),
                entry.command,
                entry.action_executed,
                entry.agent,
                entry.shell_id.map(|u| u.to_string()),
                entry.browser_id.map(|u| u.to_string()),
                entry.approval_id.map(|u| u.to_string()),
                entry.result,
                entry.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_audit(&self, session_id: Option<Uuid>, limit: usize) -> Result<Vec<DiscordAuditEntry>> {
        let mut sql = String::from(
            "SELECT id, discord_user_id, bunny_user_id, guild_id, channel_id, thread_id, session_id,
             command, action_executed, agent, shell_id, browser_id, approval_id, result, created_at
             FROM discord_audit_log",
        );
        if session_id.is_some() {
            sql.push_str(" WHERE session_id = ?1");
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT ?2");
        let mut stmt = self.conn.prepare(&sql)?;
        let map_row = |r: &rusqlite::Row<'_>| {
            Ok(DiscordAuditEntry {
                id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
                discord_user_id: r.get(1)?,
                bunny_user_id: r
                    .get::<_, Option<String>>(2)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                guild_id: r.get(3)?,
                channel_id: r.get(4)?,
                thread_id: r.get(5)?,
                session_id: r
                    .get::<_, Option<String>>(6)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                command: r.get(7)?,
                action_executed: r.get(8)?,
                agent: r.get(9)?,
                shell_id: r
                    .get::<_, Option<String>>(10)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                browser_id: r
                    .get::<_, Option<String>>(11)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                approval_id: r
                    .get::<_, Option<String>>(12)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                result: r.get(13)?,
                created_at: parse_ts(&r.get::<_, String>(14)?),
            })
        };
        let rows = if let Some(sid) = session_id {
            stmt.query_map(params![sid.to_string(), limit as i64], map_row)?
        } else {
            stmt.query_map(params![limit as i64], map_row)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn create_task(&self, task: &AgentTask) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO agent_tasks (id, session_id, source, discord_thread_id, requested_by_discord_id,
               requested_by_user_id, agent, mode, status, prompt, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"#,
            params![
                task.id.to_string(),
                task.session_id.to_string(),
                task.source,
                task.discord_thread_id,
                task.requested_by_discord_id,
                task.requested_by_user_id.map(|u| u.to_string()),
                task.agent,
                mode_str(task.mode),
                status_str(task.status),
                task.prompt,
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_task_status(&self, id: Uuid, status: AgentTaskStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status_str(status), Utc::now().to_rfc3339(), id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: Uuid) -> Result<Option<AgentTask>> {
        self.conn
            .query_row(
                "SELECT id, session_id, source, discord_thread_id, requested_by_discord_id, requested_by_user_id,
                 agent, mode, status, prompt, created_at, updated_at FROM agent_tasks WHERE id = ?1",
                params![id.to_string()],
                map_task_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn bind_thread(&self, binding: &DiscordThreadBinding) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_thread_bindings (
                guild_id, channel_id, thread_id, session_id, task_id, default_shell_id, term_id,
                project_cwd, status, goal_text, git_enabled, base_branch, thread_branch, start_commit,
                last_pane_marker, last_pane_snapshot, last_input_discord_message_id, claude_session_id, created_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
            ON CONFLICT(thread_id) DO UPDATE SET
                term_id=excluded.term_id,
                project_cwd=excluded.project_cwd,
                status=excluded.status,
                goal_text=excluded.goal_text,
                git_enabled=excluded.git_enabled,
                base_branch=excluded.base_branch,
                thread_branch=excluded.thread_branch,
                start_commit=excluded.start_commit,
                last_pane_marker=excluded.last_pane_marker,
                last_pane_snapshot=excluded.last_pane_snapshot,
                last_input_discord_message_id=excluded.last_input_discord_message_id,
                claude_session_id=excluded.claude_session_id"#,
            params![
                binding.guild_id,
                binding.channel_id,
                binding.thread_id,
                binding.session_id.to_string(),
                binding.task_id.to_string(),
                binding.term_id.to_string(),
                binding.term_id.to_string(),
                binding.project_cwd,
                binding.status.as_str(),
                binding.goal_text,
                binding.git_enabled as i32,
                binding.base_branch,
                binding.thread_branch,
                binding.start_commit,
                binding.last_pane_marker as i64,
                binding.last_pane_snapshot,
                binding.last_input_discord_message_id,
                binding.claude_session_id,
                binding.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_thread_binding(&self, thread_id: &str) -> Result<Option<DiscordThreadBinding>> {
        self.conn
            .query_row(
                r#"SELECT guild_id, channel_id, thread_id, session_id, task_id,
                    COALESCE(term_id, default_shell_id), project_cwd, status, goal_text,
                    git_enabled, base_branch, thread_branch, start_commit,
                    last_pane_marker, last_pane_snapshot, last_input_discord_message_id,
                    claude_session_id, created_at
                 FROM discord_thread_bindings WHERE thread_id = ?1"#,
                params![thread_id],
                map_thread_binding_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Active thread still using this terminal (blocks manual shell close).
    pub fn get_active_thread_binding_for_term(
        &self,
        term_id: Uuid,
    ) -> Result<Option<DiscordThreadBinding>> {
        self.conn
            .query_row(
                r#"SELECT guild_id, channel_id, thread_id, session_id, task_id,
                    COALESCE(term_id, default_shell_id), project_cwd, status, goal_text,
                    git_enabled, base_branch, thread_branch, start_commit,
                    last_pane_marker, last_pane_snapshot, last_input_discord_message_id,
                    claude_session_id, created_at
                 FROM discord_thread_bindings
                 WHERE status = 'active' AND COALESCE(term_id, default_shell_id) = ?1
                 LIMIT 1"#,
                params![term_id.to_string()],
                map_thread_binding_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn update_thread_status(
        &self,
        thread_id: &str,
        status: DiscordThreadStatus,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_thread_bindings SET status = ?1 WHERE thread_id = ?2",
            params![status.as_str(), thread_id],
        )?;
        Ok(())
    }

    pub fn update_thread_pane_marker(&self, thread_id: &str, marker: usize) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_thread_bindings SET last_pane_marker = ?1 WHERE thread_id = ?2",
            params![marker as i64, thread_id],
        )?;
        Ok(())
    }

    pub fn update_thread_pane_snapshot(&self, thread_id: &str, snapshot: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_thread_bindings SET last_pane_snapshot = ?1, last_pane_marker = ?2 WHERE thread_id = ?3",
            params![snapshot, snapshot.chars().count() as i64, thread_id],
        )?;
        Ok(())
    }

    pub fn set_thread_last_input_message(
        &self,
        thread_id: &str,
        discord_message_id: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_thread_bindings SET last_input_discord_message_id = ?1 WHERE thread_id = ?2",
            params![discord_message_id, thread_id],
        )?;
        Ok(())
    }

    pub fn set_thread_goal_text(&self, thread_id: &str, goal: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_thread_bindings SET goal_text = ?1 WHERE thread_id = ?2",
            params![goal, thread_id],
        )?;
        Ok(())
    }

    pub fn get_thread_claude_session_id(&self, thread_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT claude_session_id FROM discord_thread_bindings WHERE thread_id = ?1",
                params![thread_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|opt| opt.flatten())
            .map_err(Into::into)
    }

    pub fn set_thread_claude_session_id(
        &self,
        thread_id: &str,
        session_id: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_thread_bindings SET claude_session_id = ?1 WHERE thread_id = ?2",
            params![session_id, thread_id],
        )?;
        Ok(())
    }

    pub fn cancel_thread_pending_questions(&self, thread_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM discord_thread_pending_questions WHERE thread_id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    pub fn insert_thread_pending_questions(
        &self,
        pending: &DiscordThreadPendingQuestions,
    ) -> Result<()> {
        self.cancel_thread_pending_questions(&pending.thread_id)?;
        let questions_json = serde_json::to_string(&pending.questions)?;
        let answers_json = serde_json::to_string(&pending.answers)?;
        self.conn.execute(
            r#"INSERT INTO discord_thread_pending_questions (id, thread_id, questions_json, answers_json, created_at)
               VALUES (?1,?2,?3,?4,?5)"#,
            params![
                pending.id.to_string(),
                pending.thread_id,
                questions_json,
                answers_json,
                pending.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_thread_pending_questions(
        &self,
        pending_id: Uuid,
    ) -> Result<Option<DiscordThreadPendingQuestions>> {
        self.conn
            .query_row(
                "SELECT id, thread_id, questions_json, answers_json, created_at FROM discord_thread_pending_questions WHERE id = ?1",
                params![pending_id.to_string()],
                map_thread_pending_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_active_thread_pending_questions(
        &self,
        thread_id: &str,
    ) -> Result<Option<DiscordThreadPendingQuestions>> {
        self.conn
            .query_row(
                "SELECT id, thread_id, questions_json, answers_json, created_at FROM discord_thread_pending_questions WHERE thread_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![thread_id],
                map_thread_pending_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn update_thread_pending_answers(
        &self,
        pending_id: Uuid,
        answers: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let answers_json = serde_json::to_string(answers)?;
        self.conn.execute(
            "UPDATE discord_thread_pending_questions SET answers_json = ?1 WHERE id = ?2",
            params![answers_json, pending_id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete_thread_pending_questions(&self, pending_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM discord_thread_pending_questions WHERE id = ?1",
            params![pending_id.to_string()],
        )?;
        Ok(())
    }

    pub fn cancel_thread_pending_permissions(&self, thread_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM discord_thread_pending_permissions WHERE thread_id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    pub fn insert_thread_pending_permission(
        &self,
        pending: &DiscordThreadPendingPermission,
    ) -> Result<()> {
        self.cancel_thread_pending_permissions(&pending.thread_id)?;
        let allowed_json = serde_json::to_string(&pending.allowed_tools)?;
        self.conn.execute(
            r#"INSERT INTO discord_thread_pending_permissions (id, thread_id, claude_session_id, command, allowed_tools_json, created_at)
               VALUES (?1,?2,?3,?4,?5,?6)"#,
            params![
                pending.id.to_string(),
                pending.thread_id,
                pending.claude_session_id,
                pending.command,
                allowed_json,
                pending.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_thread_pending_permission(
        &self,
        pending_id: Uuid,
    ) -> Result<Option<DiscordThreadPendingPermission>> {
        self.conn
            .query_row(
                "SELECT id, thread_id, claude_session_id, command, allowed_tools_json, created_at FROM discord_thread_pending_permissions WHERE id = ?1",
                params![pending_id.to_string()],
                map_thread_pending_permission_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete_thread_pending_permission(&self, pending_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM discord_thread_pending_permissions WHERE id = ?1",
            params![pending_id.to_string()],
        )?;
        Ok(())
    }

    pub fn record_thread_denied_shell_keys(
        &self,
        thread_id: &str,
        command_keys: &[String],
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for key in command_keys {
            if key.is_empty() {
                continue;
            }
            let _ = self.conn.execute(
                r#"INSERT INTO discord_thread_denied_shell_commands (thread_id, command_key, created_at)
                   VALUES (?1, ?2, ?3)
                   ON CONFLICT(thread_id, command_key) DO NOTHING"#,
                params![thread_id, key, now],
            );
        }
        Ok(())
    }

    pub fn is_thread_shell_command_denied(
        &self,
        thread_id: &str,
        command_keys: &[String],
    ) -> Result<bool> {
        for key in command_keys {
            if key.is_empty() {
                continue;
            }
            let found: bool = self
                .conn
                .query_row(
                    "SELECT 1 FROM discord_thread_denied_shell_commands WHERE thread_id = ?1 AND command_key = ?2 LIMIT 1",
                    params![thread_id, key],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if found {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn clear_thread_denied_shell_commands(&self, thread_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM discord_thread_denied_shell_commands WHERE thread_id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    pub fn record_thread_granted_shell_keys(
        &self,
        thread_id: &str,
        command_keys: &[String],
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for key in command_keys {
            if key.is_empty() {
                continue;
            }
            let _ = self.conn.execute(
                r#"INSERT INTO discord_thread_granted_shell_commands (thread_id, command_key, created_at)
                   VALUES (?1, ?2, ?3)
                   ON CONFLICT(thread_id, command_key) DO NOTHING"#,
                params![thread_id, key, now],
            );
        }
        Ok(())
    }

    pub fn is_thread_shell_command_granted(
        &self,
        thread_id: &str,
        command_keys: &[String],
    ) -> Result<bool> {
        for key in command_keys {
            if key.is_empty() {
                continue;
            }
            let found: bool = self
                .conn
                .query_row(
                    "SELECT 1 FROM discord_thread_granted_shell_commands WHERE thread_id = ?1 AND command_key = ?2 LIMIT 1",
                    params![thread_id, key],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if found {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn insert_thread_message(&self, msg: &DiscordThreadMessage) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_thread_messages (id, thread_id, role, discord_user_id, author_name, content, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7)"#,
            params![
                msg.id.to_string(),
                msg.thread_id,
                msg.role.as_str(),
                msg.discord_user_id,
                msg.author_name,
                msg.content,
                msg.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_thread_messages(
        &self,
        thread_id: &str,
        limit: usize,
    ) -> Result<Vec<DiscordThreadMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, thread_id, role, discord_user_id, author_name, content, created_at
             FROM discord_thread_messages WHERE thread_id = ?1 ORDER BY created_at ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![thread_id, limit as i64], map_thread_message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn insert_thread_discussion(&self, entry: &DiscordThreadDiscussion) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_thread_discussion (id, thread_id, discord_user_id, author_name, content, created_at)
               VALUES (?1,?2,?3,?4,?5,?6)"#,
            params![
                entry.id.to_string(),
                entry.thread_id,
                entry.discord_user_id,
                entry.author_name,
                entry.content,
                entry.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_thread_discussion(
        &self,
        thread_id: &str,
        limit: usize,
    ) -> Result<Vec<DiscordThreadDiscussion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, thread_id, discord_user_id, author_name, content, created_at FROM discord_thread_discussion WHERE thread_id = ?1 ORDER BY created_at ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![thread_id, limit as i64], |r| {
            Ok(DiscordThreadDiscussion {
                id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
                thread_id: r.get(1)?,
                discord_user_id: r.get(2)?,
                author_name: r.get(3)?,
                content: r.get(4)?,
                created_at: parse_ts(&r.get::<_, String>(5)?),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_active_thread_ids_for_channel(
        &self,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT thread_id FROM discord_thread_bindings WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active'",
        )?;
        let rows = stmt.query_map(params![guild_id, channel_id], |r| r.get(0))?;
        rows.collect::<Result<Vec<String>, _>>().map_err(Into::into)
    }

    pub fn create_approval(&self, req: &ApprovalRequest) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO approval_requests (id, task_id, session_id, action_summary, reason, status, discord_message_id, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"#,
            params![
                req.id.to_string(),
                req.task_id.to_string(),
                req.session_id.to_string(),
                req.action_summary,
                req.reason,
                req.status,
                req.discord_message_id,
                req.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn resolve_approval(&self, id: Uuid, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE approval_requests SET status = ?1, resolved_at = ?2 WHERE id = ?3",
            params![status, Utc::now().to_rfc3339(), id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_approval(&self, id: Uuid) -> Result<Option<ApprovalRequest>> {
        self.conn
            .query_row(
                "SELECT id, task_id, session_id, action_summary, reason, status, discord_message_id, created_at, resolved_at FROM approval_requests WHERE id = ?1",
                params![id.to_string()],
                |r| {
                    Ok(ApprovalRequest {
                        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
                        task_id: Uuid::parse_str(&r.get::<_, String>(1)?).unwrap_or_default(),
                        session_id: Uuid::parse_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                        action_summary: r.get(3)?,
                        reason: r.get(4)?,
                        status: r.get(5)?,
                        discord_message_id: r.get(6)?,
                        created_at: parse_ts(&r.get::<_, String>(7)?),
                        resolved_at: r
                            .get::<_, Option<String>>(8)?
                            .map(|s| parse_ts(&s)),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn create_watch(&self, watch: &WatchSession) -> Result<()> {
        let roles = serde_json::to_string(&watch.required_role_ids)?;
        self.conn.execute(
            r#"INSERT INTO watch_sessions (id, token, session_id, guild_id, channel_id, thread_id, layout, visibility, mode, status, required_role_ids, browser_id, expires_at, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"#,
            params![
                watch.id.to_string(),
                watch.token,
                watch.session_id.to_string(),
                watch.guild_id,
                watch.channel_id,
                watch.thread_id,
                watch.layout,
                watch.visibility,
                watch.mode,
                watch.status,
                roles,
                watch.browser_id.map(|id| id.to_string()),
                watch.expires_at.to_rfc3339(),
                watch.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_watch_by_token(&self, token: &str) -> Result<Option<WatchSession>> {
        self.conn
            .query_row(
                "SELECT id, token, session_id, guild_id, channel_id, thread_id, layout, visibility, mode, status, required_role_ids, browser_id, expires_at, created_at FROM watch_sessions WHERE token = ?1 AND status = 'active'",
                params![token],
                map_watch_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn stop_watch(&self, token: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE watch_sessions SET status = 'stopped' WHERE token = ?1 AND status = 'active'",
            params![token],
        )?;
        Ok(n > 0)
    }

    pub fn active_watch_tokens_for_channel(
        &self,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<Vec<String>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT token FROM watch_sessions WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active' AND expires_at > ?3",
        )?;
        let tokens = stmt
            .query_map(params![guild_id, channel_id, now], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(tokens)
    }

    pub fn stop_all_watches_for_channel(&self, guild_id: &str, channel_id: &str) -> Result<u32> {
        let n = self.conn.execute(
            "UPDATE watch_sessions SET status = 'stopped' WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active'",
            params![guild_id, channel_id],
        )?;
        Ok(n as u32)
    }

    pub fn active_watch_for_channel(&self, guild_id: &str, channel_id: &str) -> Result<Option<WatchSession>> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .query_row(
                "SELECT id, token, session_id, guild_id, channel_id, thread_id, layout, visibility, mode, status, required_role_ids, browser_id, expires_at, created_at
                 FROM watch_sessions WHERE guild_id = ?1 AND channel_id = ?2 AND status = 'active' AND expires_at > ?3 ORDER BY created_at DESC LIMIT 1",
                params![guild_id, channel_id, now],
                map_watch_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_follow(&self, follow: &DiscordFollow) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO discord_follows (id, guild_id, channel_id, session_id, target, shell_name, interval_secs, active, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
               ON CONFLICT(id) DO UPDATE SET active=excluded.active, interval_secs=excluded.interval_secs"#,
            params![
                follow.id.to_string(),
                follow.guild_id,
                follow.channel_id,
                follow.session_id.to_string(),
                follow.target,
                follow.shell_name,
                follow.interval_secs as i64,
                follow.active as i32,
                follow.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn deactivate_follows(&self, guild_id: &str, channel_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE discord_follows SET active = 0 WHERE guild_id = ?1 AND channel_id = ?2",
            params![guild_id, channel_id],
        )?;
        Ok(())
    }

    pub fn list_active_follows(&self) -> Result<Vec<DiscordFollow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, guild_id, channel_id, session_id, target, shell_name, interval_secs, active, created_at FROM discord_follows WHERE active = 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DiscordFollow {
                id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
                guild_id: r.get(1)?,
                channel_id: r.get(2)?,
                session_id: Uuid::parse_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                target: r.get(4)?,
                shell_name: r.get(5)?,
                interval_secs: r.get::<_, i64>(6)? as u64,
                active: r.get::<_, i32>(7)? != 0,
                created_at: parse_ts(&r.get::<_, String>(8)?),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn verify_bridge_token(&self, token: &str, configured_hash: &str) -> bool {
        let hash = hash_token(token);
        hash == configured_hash
    }
}

pub fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    format!("{:x}", h.finalize())
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn mode_str(m: AgentTaskMode) -> &'static str {
    match m {
        AgentTaskMode::Ask => "ask",
        AgentTaskMode::Plan => "plan",
        AgentTaskMode::Do => "do",
        AgentTaskMode::Shell => "shell",
        AgentTaskMode::Browser => "browser",
    }
}

fn status_str(s: AgentTaskStatus) -> &'static str {
    match s {
        AgentTaskStatus::Queued => "queued",
        AgentTaskStatus::Running => "running",
        AgentTaskStatus::WaitingApproval => "waiting_approval",
        AgentTaskStatus::Done => "done",
        AgentTaskStatus::Failed => "failed",
        AgentTaskStatus::Cancelled => "cancelled",
    }
}

fn parse_mode(s: &str) -> AgentTaskMode {
    match s {
        "plan" => AgentTaskMode::Plan,
        "do" => AgentTaskMode::Do,
        "shell" => AgentTaskMode::Shell,
        "browser" => AgentTaskMode::Browser,
        _ => AgentTaskMode::Ask,
    }
}

fn parse_status(s: &str) -> AgentTaskStatus {
    match s {
        "running" => AgentTaskStatus::Running,
        "waiting_approval" => AgentTaskStatus::WaitingApproval,
        "done" => AgentTaskStatus::Done,
        "failed" => AgentTaskStatus::Failed,
        "cancelled" => AgentTaskStatus::Cancelled,
        _ => AgentTaskStatus::Queued,
    }
}

fn map_session_link_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DiscordSessionLink> {
    Ok(DiscordSessionLink {
        guild_id: r.get(0)?,
        channel_id: r.get(1)?,
        session_id: Uuid::parse_str(&r.get::<_, String>(2)?).unwrap_or_default(),
        created_by_user_id: r
            .get::<_, Option<String>>(3)?
            .and_then(|s| Uuid::parse_str(&s).ok()),
        status: r.get(4)?,
        project_cwd_override: r
            .get::<_, Option<String>>(5)?
            .filter(|s| !s.is_empty()),
        created_at: parse_ts(&r.get::<_, String>(6)?),
    })
}

fn map_thread_binding_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DiscordThreadBinding> {
    let term_raw: Option<String> = r.get(5)?;
    let term_id = term_raw
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_default();
    let status: String = r.get(7).unwrap_or_else(|_| "active".into());
    Ok(DiscordThreadBinding {
        guild_id: r.get(0)?,
        channel_id: r.get(1)?,
        thread_id: r.get(2)?,
        session_id: Uuid::parse_str(&r.get::<_, String>(3)?).unwrap_or_default(),
        task_id: Uuid::parse_str(&r.get::<_, String>(4)?).unwrap_or_default(),
        term_id,
        project_cwd: r
            .get::<_, Option<String>>(6)?
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
        status: DiscordThreadStatus::parse(&status),
        goal_text: r.get(8)?,
        git_enabled: r.get::<_, i32>(9).unwrap_or(0) != 0,
        base_branch: r.get(10)?,
        thread_branch: r.get(11)?,
        start_commit: r.get(12)?,
        last_pane_marker: r.get::<_, i64>(13).unwrap_or(0) as usize,
        last_pane_snapshot: r.get::<_, Option<String>>(14).ok().flatten().unwrap_or_default(),
        last_input_discord_message_id: r.get(15)?,
        claude_session_id: r.get::<_, Option<String>>(16).ok().flatten(),
        created_at: parse_ts(&r.get::<_, String>(17)?),
    })
}

fn map_discord_user_link_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DiscordUserLink> {
    Ok(DiscordUserLink {
        discord_user_id: r.get(0)?,
        user_id: Uuid::parse_str(&r.get::<_, String>(1)?)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        discord_username: r.get(2)?,
        discord_global_name: r.get(3)?,
        created_at: parse_ts(&r.get::<_, String>(4)?),
    })
}

fn map_thread_message_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DiscordThreadMessage> {
    Ok(DiscordThreadMessage {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
        thread_id: r.get(1)?,
        role: DiscordThreadMessageRole::parse(&r.get::<_, String>(2)?),
        discord_user_id: r.get(3)?,
        author_name: r.get(4)?,
        content: r.get(5)?,
        created_at: parse_ts(&r.get::<_, String>(6)?),
    })
}

fn map_thread_pending_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DiscordThreadPendingQuestions> {
    let questions: Vec<AskUserQuestionItem> =
        serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default();
    let answers: std::collections::HashMap<String, String> =
        serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
    Ok(DiscordThreadPendingQuestions {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
        thread_id: r.get(1)?,
        questions,
        answers,
        created_at: parse_ts(&r.get::<_, String>(4)?),
    })
}

fn map_thread_pending_permission_row(
    r: &rusqlite::Row<'_>,
) -> rusqlite::Result<DiscordThreadPendingPermission> {
    let allowed_tools: Vec<String> =
        serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default();
    Ok(DiscordThreadPendingPermission {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
        thread_id: r.get(1)?,
        claude_session_id: r.get(2)?,
        command: r.get(3)?,
        allowed_tools,
        created_at: parse_ts(&r.get::<_, String>(5)?),
    })
}

fn map_task_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTask> {
    Ok(AgentTask {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
        session_id: Uuid::parse_str(&r.get::<_, String>(1)?).unwrap_or_default(),
        source: r.get(2)?,
        discord_thread_id: r.get(3)?,
        requested_by_discord_id: r.get(4)?,
        requested_by_user_id: r
            .get::<_, Option<String>>(5)?
            .and_then(|s| Uuid::parse_str(&s).ok()),
        agent: r.get(6)?,
        mode: parse_mode(&r.get::<_, String>(7)?),
        status: parse_status(&r.get::<_, String>(8)?),
        prompt: r.get(9)?,
        created_at: parse_ts(&r.get::<_, String>(10)?),
        updated_at: parse_ts(&r.get::<_, String>(11)?),
    })
}

fn map_watch_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WatchSession> {
    let roles_json: String = r.get(10)?;
    let roles: Vec<String> = serde_json::from_str(&roles_json).unwrap_or_default();
    let browser_id: Option<String> = r.get(11)?;
    Ok(WatchSession {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
        token: r.get(1)?,
        session_id: Uuid::parse_str(&r.get::<_, String>(2)?).unwrap_or_default(),
        guild_id: r.get(3)?,
        channel_id: r.get(4)?,
        thread_id: r.get(5)?,
        layout: r.get(6)?,
        visibility: r.get(7)?,
        mode: r.get(8)?,
        status: r.get(9)?,
        required_role_ids: roles,
        browser_id: browser_id.and_then(|s| Uuid::parse_str(&s).ok()),
        expires_at: parse_ts(&r.get::<_, String>(12)?),
        created_at: parse_ts(&r.get::<_, String>(13)?),
    })
}
