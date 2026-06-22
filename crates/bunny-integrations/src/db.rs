use crate::types::*;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub struct IntegrationsDb {
    conn: Connection,
}

impl IntegrationsDb {
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chat_bridge_installations (
                bridge_id TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                config_json TEXT NOT NULL DEFAULT '{}',
                bridge_token_hash TEXT,
                installed_by TEXT,
                created_at TEXT NOT NULL,
                PRIMARY KEY (bridge_id, workspace_id)
            );

            CREATE TABLE IF NOT EXISTS chat_account_links (
                bridge_id TEXT NOT NULL,
                external_user_id TEXT NOT NULL,
                bunny_user_id TEXT NOT NULL,
                profile_json TEXT NOT NULL DEFAULT '{}',
                linked_at TEXT NOT NULL,
                PRIMARY KEY (bridge_id, external_user_id)
            );

            CREATE TABLE IF NOT EXISTS chat_session_links (
                bridge_id TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                project_cwd_override TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL,
                PRIMARY KEY (bridge_id, workspace_id, channel_id)
            );

            CREATE TABLE IF NOT EXISTS chat_link_codes (
                code TEXT PRIMARY KEY,
                bridge_id TEXT NOT NULL DEFAULT 'discord',
                session_id TEXT NOT NULL,
                created_by_user_id TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                used_at TEXT
            );

            CREATE TABLE IF NOT EXISTS conversation_bindings (
                id TEXT PRIMARY KEY,
                bridge_id TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL UNIQUE,
                session_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                term_id TEXT NOT NULL,
                project_cwd TEXT NOT NULL,
                git_lease_id TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                goal_text TEXT,
                git_enabled INTEGER NOT NULL DEFAULT 0,
                base_branch TEXT,
                thread_branch TEXT,
                start_commit TEXT,
                claude_session_id TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conversation_messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                bunny_user_id TEXT,
                author_name TEXT,
                content TEXT NOT NULL,
                external_message_ref_json TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_conv_messages ON conversation_messages(conversation_id, created_at);

            CREATE TABLE IF NOT EXISTS session_activity_feed (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                summary TEXT NOT NULL,
                ref_type TEXT,
                ref_id TEXT,
                bridge_id TEXT,
                ts TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_activity_session ON session_activity_feed(session_id, ts);

            CREATE TABLE IF NOT EXISTS chat_audit_log (
                id TEXT PRIMARY KEY,
                bridge_id TEXT,
                external_user_id TEXT,
                bunny_user_id TEXT,
                workspace_id TEXT,
                channel_id TEXT,
                conversation_id TEXT,
                session_id TEXT,
                command TEXT NOT NULL,
                action_executed TEXT NOT NULL,
                approval_id TEXT,
                result TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS integration_installations (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                external_org_id TEXT,
                config_json TEXT NOT NULL DEFAULT '{}',
                installed_by TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS integration_account_links (
                provider TEXT NOT NULL,
                external_user_id TEXT NOT NULL,
                bunny_user_id TEXT NOT NULL,
                profile_json TEXT NOT NULL DEFAULT '{}',
                linked_at TEXT NOT NULL,
                PRIMARY KEY (provider, external_user_id)
            );

            CREATE TABLE IF NOT EXISTS integration_oauth_tokens (
                id TEXT PRIMARY KEY,
                installation_id TEXT NOT NULL,
                bunny_user_id TEXT,
                scopes TEXT NOT NULL DEFAULT '[]',
                token_enc TEXT NOT NULL,
                refresh_enc TEXT,
                expires_at TEXT
            );

            CREATE TABLE IF NOT EXISTS integration_resource_bindings (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                installation_id TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_ref TEXT NOT NULL,
                config_json TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS integration_permission_cache (
                id TEXT PRIMARY KEY,
                account_link_provider TEXT NOT NULL,
                account_link_external_user_id TEXT NOT NULL,
                resource_binding_id TEXT NOT NULL,
                capabilities_json TEXT NOT NULL,
                synced_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS git_repo_bindings (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL,
                local_path TEXT,
                remote_url TEXT,
                default_branch TEXT NOT NULL DEFAULT 'main',
                mirror_path TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS git_worktree_leases (
                id TEXT PRIMARY KEY,
                repo_binding_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                context_id TEXT,
                branch TEXT NOT NULL,
                worktree_path TEXT NOT NULL,
                base_commit TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL,
                released_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_git_leases_session ON git_worktree_leases(session_id, status);

            CREATE TABLE IF NOT EXISTS action_definitions (
                id TEXT PRIMARY KEY,
                provider TEXT,
                action_id TEXT NOT NULL,
                risk TEXT NOT NULL,
                approval_policy_json TEXT
            );

            CREATE TABLE IF NOT EXISTS proposed_actions (
                id TEXT PRIMARY KEY,
                task_id TEXT,
                action_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                requested_by TEXT NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS integration_audit_log (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                session_id TEXT,
                user_id TEXT,
                action_id TEXT NOT NULL,
                result TEXT NOT NULL,
                payload_json TEXT,
                created_at TEXT NOT NULL
            );
            "#,
        )?;

        // Extend approval_requests if managed by discord db - we add columns via integrations migration
        let _ = self.conn.execute(
            "ALTER TABLE approval_requests ADD COLUMN approver_policy_json TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE approval_requests ADD COLUMN channels_notified TEXT NOT NULL DEFAULT '[]'",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE approval_requests ADD COLUMN resolved_by_user_id TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE approval_requests ADD COLUMN proposed_action_id TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE approval_requests ADD COLUMN source_bridge TEXT",
            [],
        );

        Ok(())
    }

    pub fn insert_conversation_message(&self, msg: &ConversationMessage) -> Result<()> {
        let ext = msg
            .external_message_ref
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.conn.execute(
            r#"INSERT INTO conversation_messages
               (id, conversation_id, role, bunny_user_id, author_name, content, external_message_ref_json, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"#,
            params![
                msg.id.to_string(),
                msg.conversation_id,
                msg.role.as_str(),
                msg.bunny_user_id.map(|u| u.to_string()),
                msg.author_name,
                msg.content,
                ext,
                msg.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_conversation_messages(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, conversation_id, role, bunny_user_id, author_name, content,
                      external_message_ref_json, created_at
               FROM conversation_messages WHERE conversation_id = ?1
               ORDER BY created_at ASC LIMIT ?2"#,
        )?;
        let rows = stmt.query_map(params![conversation_id, limit as i64], |row| {
            let ext: Option<String> = row.get(6)?;
            Ok(ConversationMessage {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                conversation_id: row.get(1)?,
                role: ConversationMessageRole::parse(&row.get::<_, String>(2)?),
                bunny_user_id: row
                    .get::<_, Option<String>>(3)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                author_name: row.get(4)?,
                content: row.get(5)?,
                external_message_ref: ext.and_then(|j| serde_json::from_str(&j).ok()),
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_activity(&self, entry: &SessionActivityEntry) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO session_activity_feed
               (id, session_id, kind, summary, ref_type, ref_id, bridge_id, ts)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"#,
            params![
                entry.id.to_string(),
                entry.session_id.to_string(),
                entry.kind,
                entry.summary,
                entry.ref_type,
                entry.ref_id,
                entry.bridge_id,
                entry.ts.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_activity(&self, session_id: Uuid, limit: usize) -> Result<Vec<SessionActivityEntry>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, session_id, kind, summary, ref_type, ref_id, bridge_id, ts
               FROM session_activity_feed WHERE session_id = ?1 ORDER BY ts DESC LIMIT ?2"#,
        )?;
        let rows = stmt.query_map(params![session_id.to_string(), limit as i64], |row| {
            Ok(SessionActivityEntry {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                session_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_else(|_| Uuid::nil()),
                kind: row.get(2)?,
                summary: row.get(3)?,
                ref_type: row.get(4)?,
                ref_id: row.get(5)?,
                bridge_id: row.get(6)?,
                ts: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_git_repo_binding(
        &self,
        id: Uuid,
        session_id: Uuid,
        source: &str,
        local_path: Option<&str>,
        remote_url: Option<&str>,
        default_branch: &str,
        mirror_path: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO git_repo_bindings (id, session_id, source, local_path, remote_url, default_branch, mirror_path, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
               ON CONFLICT(id) DO UPDATE SET
                 source=excluded.source, local_path=excluded.local_path, remote_url=excluded.remote_url,
                 default_branch=excluded.default_branch, mirror_path=excluded.mirror_path"#,
            params![
                id.to_string(),
                session_id.to_string(),
                source,
                local_path,
                remote_url,
                default_branch,
                mirror_path,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_git_repo_binding_for_session(&self, session_id: Uuid) -> Result<Option<(Uuid, String, Option<String>, Option<String>, String, Option<String>)>> {
        self.conn
            .query_row(
                r#"SELECT id, source, local_path, remote_url, default_branch, mirror_path
                   FROM git_repo_bindings WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1"#,
                params![session_id.to_string()],
                |row| {
                    Ok((
                        Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_git_lease(&self, lease: &GitWorktreeLease) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO git_worktree_leases
               (id, repo_binding_id, session_id, context_id, branch, worktree_path, base_commit, status, created_at, released_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
            params![
                lease.id.to_string(),
                lease.repo_binding_id.to_string(),
                lease.session_id.to_string(),
                lease.context_id,
                lease.branch,
                lease.worktree_path.to_string_lossy().to_string(),
                lease.base_commit,
                lease.status.as_str(),
                lease.created_at.to_rfc3339(),
                lease.released_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn update_git_lease_status(
        &self,
        id: Uuid,
        status: GitLeaseStatus,
        released_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE git_worktree_leases SET status = ?1, released_at = ?2 WHERE id = ?3",
            params![
                status.as_str(),
                released_at.map(|t| t.to_rfc3339()),
                id.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn get_active_lease_by_context(
        &self,
        context_id: &str,
    ) -> Result<Option<GitWorktreeLease>> {
        self.conn
            .query_row(
                r#"SELECT id, repo_binding_id, session_id, context_id, branch, worktree_path,
                          base_commit, status, created_at, released_at
                   FROM git_worktree_leases WHERE context_id = ?1 AND status = 'active' LIMIT 1"#,
                params![context_id],
                map_git_lease_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_git_lease(&self, id: Uuid) -> Result<Option<GitWorktreeLease>> {
        self.conn
            .query_row(
                r#"SELECT id, repo_binding_id, session_id, context_id, branch, worktree_path,
                          base_commit, status, created_at, released_at
                   FROM git_worktree_leases WHERE id = ?1"#,
                params![id.to_string()],
                map_git_lease_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_proposed_action(&self, rec: &ProposedActionRecord) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO proposed_actions
               (id, task_id, action_id, session_id, requested_by, payload_json, status, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"#,
            params![
                rec.id.to_string(),
                rec.task_id.map(|t| t.to_string()),
                rec.action_id,
                rec.session_id.to_string(),
                rec.requested_by.to_string(),
                rec.payload_json,
                rec.status,
                rec.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_proposed_action_status(&self, id: Uuid, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE proposed_actions SET status = ?1 WHERE id = ?2",
            params![status, id.to_string()],
        )?;
        Ok(())
    }

    pub fn insert_integration_account_link(
        &self,
        provider: &str,
        external_user_id: &str,
        bunny_user_id: Uuid,
        profile_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO integration_account_links (provider, external_user_id, bunny_user_id, profile_json, linked_at)
               VALUES (?1,?2,?3,?4,?5)
               ON CONFLICT(provider, external_user_id) DO UPDATE SET
                 bunny_user_id=excluded.bunny_user_id, profile_json=excluded.profile_json"#,
            params![
                provider,
                external_user_id,
                bunny_user_id.to_string(),
                profile_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_integration_account_link(
        &self,
        provider: &str,
        bunny_user_id: Uuid,
    ) -> Result<Option<(String, String)>> {
        self.conn
            .query_row(
                r#"SELECT external_user_id, profile_json FROM integration_account_links
                   WHERE provider = ?1 AND bunny_user_id = ?2 LIMIT 1"#,
                params![provider, bunny_user_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_resource_binding(&self, binding: &ResourceBinding) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO integration_resource_bindings
               (id, session_id, installation_id, resource_type, resource_ref, config_json)
               VALUES (?1,?2,?3,?4,?5,?6)
               ON CONFLICT(id) DO UPDATE SET resource_type=excluded.resource_type,
                 resource_ref=excluded.resource_ref, config_json=excluded.config_json"#,
            params![
                binding.id.to_string(),
                binding.session_id.to_string(),
                binding.installation_id.to_string(),
                binding.resource_type,
                binding.resource_ref,
                serde_json::to_string(&binding.config)?,
            ],
        )?;
        Ok(())
    }

    pub fn list_resource_bindings(&self, session_id: Uuid) -> Result<Vec<ResourceBinding>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, session_id, installation_id, resource_type, resource_ref, config_json
               FROM integration_resource_bindings WHERE session_id = ?1"#,
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |row| {
            let config: String = row.get(5)?;
            Ok(ResourceBinding {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                session_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_else(|_| Uuid::nil()),
                installation_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap_or_else(|_| Uuid::nil()),
                resource_type: row.get(3)?,
                resource_ref: row.get(4)?,
                config: serde_json::from_str(&config).unwrap_or(serde_json::json!({})),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn cache_permissions(
        &self,
        id: Uuid,
        provider: &str,
        external_user_id: &str,
        resource_binding_id: Uuid,
        capabilities_json: &str,
        ttl_minutes: i64,
    ) -> Result<()> {
        let now = Utc::now();
        let expires = now + Duration::minutes(ttl_minutes);
        self.conn.execute(
            r#"INSERT INTO integration_permission_cache
               (id, account_link_provider, account_link_external_user_id, resource_binding_id,
                capabilities_json, synced_at, expires_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7)
               ON CONFLICT(id) DO UPDATE SET capabilities_json=excluded.capabilities_json,
                 synced_at=excluded.synced_at, expires_at=excluded.expires_at"#,
            params![
                id.to_string(),
                provider,
                external_user_id,
                resource_binding_id.to_string(),
                capabilities_json,
                now.to_rfc3339(),
                expires.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_cached_permissions(
        &self,
        provider: &str,
        external_user_id: &str,
        resource_binding_id: Uuid,
    ) -> Result<Option<(String, DateTime<Utc>)>> {
        self.conn
            .query_row(
                r#"SELECT capabilities_json, expires_at FROM integration_permission_cache
                   WHERE account_link_provider = ?1 AND account_link_external_user_id = ?2
                     AND resource_binding_id = ?3 AND expires_at > ?4
                   ORDER BY synced_at DESC LIMIT 1"#,
                params![
                    provider,
                    external_user_id,
                    resource_binding_id.to_string(),
                    Utc::now().to_rfc3339(),
                ],
                |row| {
                    let exp: String = row.get(1)?;
                    Ok((
                        row.get(0)?,
                        DateTime::parse_from_rfc3339(&exp)
                            .map(|d| d.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    ))
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_chat_account_link(
        &self,
        bridge_id: &str,
        external_user_id: &str,
        bunny_user_id: Uuid,
        profile_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO chat_account_links (bridge_id, external_user_id, bunny_user_id, profile_json, linked_at)
               VALUES (?1,?2,?3,?4,?5)
               ON CONFLICT(bridge_id, external_user_id) DO UPDATE SET bunny_user_id=excluded.bunny_user_id"#,
            params![
                bridge_id,
                external_user_id,
                bunny_user_id.to_string(),
                profile_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_chat_account_link(
        &self,
        bridge_id: &str,
        external_user_id: &str,
    ) -> Result<Option<Uuid>> {
        self.conn
            .query_row(
                "SELECT bunny_user_id FROM chat_account_links WHERE bridge_id = ?1 AND external_user_id = ?2",
                params![bridge_id, external_user_id],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(Uuid::parse_str(&s).unwrap_or_else(|_| Uuid::nil()))
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_session_chat_links(&self, session_id: Uuid) -> Result<Vec<ChatSessionLink>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT bridge_id, workspace_id, channel_id, session_id, project_cwd_override, status, created_at
               FROM chat_session_links WHERE session_id = ?1 AND status = 'active'"#,
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |row| {
            Ok(ChatSessionLink {
                bridge_id: row.get(0)?,
                workspace_id: row.get(1)?,
                channel_id: row.get(2)?,
                session_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap_or_else(|_| Uuid::nil()),
                project_cwd_override: row.get(4)?,
                status: row.get(5)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_approval_policy(&self, approval_id: Uuid) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT approver_policy_json FROM approval_requests WHERE id = ?1",
                params![approval_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_approval_policy(&self, approval_id: Uuid, policy_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE approval_requests SET approver_policy_json = ?1 WHERE id = ?2",
            params![policy_json, approval_id.to_string()],
        )?;
        Ok(())
    }

    pub fn set_approval_channels_notified(&self, approval_id: Uuid, channels_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE approval_requests SET channels_notified = ?1 WHERE id = ?2",
            params![channels_json, approval_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_pending_approvals(&self, session_id: Uuid) -> Result<Vec<(Uuid, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, action_summary, reason, status FROM approval_requests
               WHERE session_id = ?1 AND status = 'pending' ORDER BY created_at DESC"#,
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_activity(&self, entry: &SessionActivityEntry) -> Result<()> {
        self.insert_activity(entry)
    }

    pub fn migrate_from_discord(&self) -> Result<u32> {
        let mut count = 0u32;
        // Copy discord_user_links -> chat_account_links
        let mut stmt = self.conn.prepare(
            "SELECT discord_user_id, user_id, COALESCE(discord_username, ''), created_at FROM discord_user_links",
        )?;
        let rows: Vec<(String, String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
            .filter_map(|r| r.ok())
            .collect();
        for (ext_id, user_id, username, linked_at) in rows {
            let profile = serde_json::json!({ "username": username }).to_string();
            self.conn.execute(
                r#"INSERT OR IGNORE INTO chat_account_links (bridge_id, external_user_id, bunny_user_id, profile_json, linked_at)
                   VALUES ('discord', ?1, ?2, ?3, ?4)"#,
                params![ext_id, user_id, profile, linked_at],
            )?;
            count += 1;
        }

        let mut stmt = self.conn.prepare(
            r#"SELECT guild_id, channel_id, session_id, project_cwd_override, status, created_at
               FROM discord_session_links"#,
        )?;
        let links: Vec<(String, String, String, Option<String>, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (guild, channel, session, cwd, status, created) in links {
            self.conn.execute(
                r#"INSERT OR IGNORE INTO chat_session_links
                   (bridge_id, workspace_id, channel_id, session_id, project_cwd_override, status, created_at)
                   VALUES ('discord', ?1, ?2, ?3, ?4, ?5, ?6)"#,
                params![guild, channel, session, cwd, status, created],
            )?;
        }

        Ok(count)
    }
}

fn map_git_lease_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GitWorktreeLease> {
    Ok(GitWorktreeLease {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
        repo_binding_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_else(|_| Uuid::nil()),
        session_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap_or_else(|_| Uuid::nil()),
        context_id: row.get(3)?,
        branch: row.get(4)?,
        worktree_path: std::path::PathBuf::from(row.get::<_, String>(5)?),
        base_commit: row.get(6)?,
        status: GitLeaseStatus::parse(&row.get::<_, String>(7)?),
        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        released_at: row
            .get::<_, Option<String>>(9)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc)),
    })
}

#[derive(Debug, Clone)]
pub struct GitWorktreeLease {
    pub id: Uuid,
    pub repo_binding_id: Uuid,
    pub session_id: Uuid,
    pub context_id: Option<String>,
    pub branch: String,
    pub worktree_path: std::path::PathBuf,
    pub base_commit: String,
    pub status: GitLeaseStatus,
    pub created_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLeaseStatus {
    Active,
    Stale,
    Released,
}

impl GitLeaseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Released => "released",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "stale" => Self::Stale,
            "released" => Self::Released,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AcquireGitLeaseRequest {
    pub session_id: Uuid,
    pub context_id: Option<String>,
    pub branch: String,
    pub base_ref: Option<String>,
    pub local_path: Option<std::path::PathBuf>,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
}
