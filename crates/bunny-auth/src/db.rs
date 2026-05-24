use anyhow::Result;
use bunny_core::types::Role;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

pub struct AuthDb {
    conn: Connection,
}

impl AuthDb {
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
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                disabled_at TEXT
            );
            CREATE TABLE IF NOT EXISTS auth_sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                token_hash TEXT NOT NULL,
                device_id TEXT,
                expires_at TEXT NOT NULL,
                revoked_at TEXT,
                FOREIGN KEY(user_id) REFERENCES users(id)
            );
            CREATE TABLE IF NOT EXISTS refresh_tokens (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                token_hash TEXT NOT NULL,
                device_id TEXT,
                expires_at TEXT NOT NULL,
                rotated_at TEXT,
                FOREIGN KEY(user_id) REFERENCES users(id)
            );
            CREATE TABLE IF NOT EXISTS stream_sessions (
                id TEXT PRIMARY KEY,
                owner_id TEXT NOT NULL,
                project_path TEXT NOT NULL,
                status TEXT NOT NULL,
                config_json TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT,
                FOREIGN KEY(owner_id) REFERENCES users(id)
            );
            CREATE TABLE IF NOT EXISTS session_members (
                session_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                role TEXT NOT NULL,
                PRIMARY KEY(session_id, user_id)
            );
            CREATE TABLE IF NOT EXISTS invitations (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                email TEXT NOT NULL,
                role TEXT NOT NULL,
                token_hash TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                accepted_at TEXT
            );
            CREATE TABLE IF NOT EXISTS terminals (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                name TEXT NOT NULL,
                shell TEXT NOT NULL,
                init_command TEXT,
                status TEXT NOT NULL,
                cols INTEGER NOT NULL,
                rows INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS browser_sessions (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                target_url TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS previews (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                local_port INTEGER NOT NULL,
                public_path TEXT NOT NULL,
                status TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS timeline_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload_redacted_json TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                ts TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_logs (
                id TEXT PRIMARY KEY,
                actor_id TEXT,
                action TEXT NOT NULL,
                resource TEXT NOT NULL,
                metadata_redacted TEXT,
                ts TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS secret_refs (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                name TEXT NOT NULL,
                provider TEXT NOT NULL,
                ref_id TEXT NOT NULL,
                session_id TEXT
            );
            CREATE TABLE IF NOT EXISTS push_devices (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                platform TEXT NOT NULL,
                provider TEXT NOT NULL,
                token TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(user_id, device_id)
            );
            CREATE INDEX IF NOT EXISTS idx_timeline_session ON timeline_events(session_id, sequence);
            CREATE INDEX IF NOT EXISTS idx_push_user ON push_devices(user_id);
            "#,
        )?;
        let _ = self.conn.execute(
            "ALTER TABLE terminals ADD COLUMN cwd TEXT NOT NULL DEFAULT '/'",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE terminals ADD COLUMN tmux_target TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE stream_sessions ADD COLUMN name TEXT",
            [],
        );
        Ok(())
    }

    pub fn first_user_id(&self) -> Result<Option<Uuid>> {
        let mut stmt = self.conn.prepare("SELECT id FROM users LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Uuid::parse_str(&row.get::<_, String>(0)?)?))
        } else {
            Ok(None)
        }
    }

    pub fn user_count(&self) -> Result<u64> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(count)
    }

    pub fn create_user(&self, id: Uuid, email: &str, password_hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO users (id, email, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                id.to_string(),
                email,
                password_hash,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn find_user_by_email(&self, email: &str) -> Result<Option<(Uuid, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, password_hash FROM users WHERE email = ?1 AND disabled_at IS NULL",
        )?;
        let mut rows = stmt.query(params![email])?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let hash: String = row.get(1)?;
            Ok(Some((Uuid::parse_str(&id)?, hash)))
        } else {
            Ok(None)
        }
    }

    pub fn find_user_by_id(&self, id: Uuid) -> Result<Option<(String, DateTime<Utc>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT email, created_at FROM users WHERE id = ?1")?;
        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            let email: String = row.get(0)?;
            let created: String = row.get(1)?;
            Ok(Some((email, created.parse()?)))
        } else {
            Ok(None)
        }
    }

    pub fn create_auth_session(
        &self,
        id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        device_id: Option<&str>,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO auth_sessions (id, user_id, token_hash, device_id, expires_at) VALUES (?1,?2,?3,?4,?5)",
            params![
                id.to_string(),
                user_id.to_string(),
                token_hash,
                device_id,
                expires_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn find_session_by_token_hash(&self, token_hash: &str) -> Result<Option<(Uuid, Uuid)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, user_id FROM auth_sessions WHERE token_hash = ?1 AND revoked_at IS NULL AND expires_at > ?2",
        )?;
        let now = Utc::now().to_rfc3339();
        let mut rows = stmt.query(params![token_hash, now])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                Uuid::parse_str(&row.get::<_, String>(0)?)?,
                Uuid::parse_str(&row.get::<_, String>(1)?)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn revoke_session(&self, token_hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE auth_sessions SET revoked_at = ?1 WHERE token_hash = ?2",
            params![Utc::now().to_rfc3339(), token_hash],
        )?;
        Ok(())
    }

    pub fn get_member_role(&self, session_id: Uuid, user_id: Uuid) -> Result<Option<Role>> {
        let mut stmt = self.conn.prepare(
            "SELECT role FROM session_members WHERE session_id = ?1 AND user_id = ?2",
        )?;
        let mut rows = stmt.query(params![session_id.to_string(), user_id.to_string()])?;
        if let Some(row) = rows.next()? {
            let role: String = row.get(0)?;
            Ok(bunny_core::permissions::parse_role(&role))
        } else {
            Ok(None)
        }
    }

    pub fn add_session_member(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        role: Role,
    ) -> Result<()> {
        let role_str = role_to_str(role);
        self.conn.execute(
            "INSERT OR REPLACE INTO session_members (session_id, user_id, role) VALUES (?1,?2,?3)",
            params![session_id.to_string(), user_id.to_string(), role_str],
        )?;
        Ok(())
    }

    pub fn create_stream_session(
        &self,
        id: Uuid,
        owner_id: Uuid,
        project_path: &str,
        name: Option<&str>,
        status: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO stream_sessions (id, owner_id, project_path, name, status, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                id.to_string(),
                owner_id.to_string(),
                project_path,
                name,
                status,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn update_stream_session_name(&self, id: Uuid, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE stream_sessions SET name = ?1 WHERE id = ?2",
            params![name, id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_all_stream_sessions(&self) -> Result<Vec<(Uuid, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, project_path, status FROM stream_sessions ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn update_stream_session_status(&self, id: Uuid, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE stream_sessions SET status = ?1 WHERE id = ?2",
            params![status, id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete_stream_session(&self, id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM stream_sessions WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete_terminals_for_session(&self, session_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM terminals WHERE session_id = ?1",
            params![session_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_stream_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<String>, String, String)>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT s.id, s.name, s.project_path, s.status FROM stream_sessions s
               INNER JOIN session_members m ON m.session_id = s.id
               WHERE m.user_id = ?1 ORDER BY s.created_at DESC"#,
        )?;
        let rows = stmt.query_map(params![user_id.to_string()], |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_audit(
        &self,
        id: Uuid,
        actor_id: Option<Uuid>,
        action: &str,
        resource: &str,
        metadata: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO audit_logs (id, actor_id, action, resource, metadata_redacted, ts) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                id.to_string(),
                actor_id.map(|u| u.to_string()),
                action,
                resource,
                metadata,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn insert_timeline_event(
        &self,
        id: Uuid,
        session_id: Uuid,
        source: &str,
        event_type: &str,
        payload: &str,
        sequence: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO timeline_events (id, session_id, source, event_type, payload_redacted_json, sequence, ts) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                id.to_string(),
                session_id.to_string(),
                source,
                event_type,
                payload,
                sequence,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn next_timeline_sequence(&self, session_id: Uuid) -> Result<u64> {
        let seq: u64 = self.conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM timeline_events WHERE session_id = ?1",
            params![session_id.to_string()],
            |r| r.get(0),
        )?;
        Ok(seq)
    }

    pub fn insert_invitation(
        &self,
        id: Uuid,
        session_id: Uuid,
        email: &str,
        role: Role,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO invitations (id, session_id, email, role, token_hash, expires_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                id.to_string(),
                session_id.to_string(),
                email,
                role_to_str(role),
                token_hash,
                expires_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_timeline(
        &self,
        session_id: Uuid,
        since: u64,
        limit: u64,
    ) -> Result<Vec<(Uuid, String, String, String, u64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, event_type, payload_redacted_json, sequence, ts FROM timeline_events WHERE session_id = ?1 AND sequence >= ?2 ORDER BY sequence ASC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![session_id.to_string(), since, limit], |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get::<_, i64>(4)? as u64,
                row.get(5)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_secret_ref(
        &self,
        id: Uuid,
        scope: &str,
        name: &str,
        provider: &str,
        ref_id: &str,
        session_id: Option<Uuid>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO secret_refs (id, scope, name, provider, ref_id, session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET scope=?2, name=?3, provider=?4, ref_id=?5, session_id=?6",
            params![
                id.to_string(),
                scope,
                name,
                provider,
                ref_id,
                session_id.map(|u| u.to_string())
            ],
        )?;
        Ok(())
    }

    pub fn delete_secret_ref(&self, name: &str, scope: &str, session_id: Option<Uuid>) -> Result<()> {
        match session_id {
            Some(sid) => self.conn.execute(
                "DELETE FROM secret_refs WHERE name = ?1 AND scope = ?2 AND session_id = ?3",
                params![name, scope, sid.to_string()],
            )?,
            None => self.conn.execute(
                "DELETE FROM secret_refs WHERE name = ?1 AND scope = ?2 AND session_id IS NULL",
                params![name, scope],
            )?,
        };
        Ok(())
    }

    pub fn upsert_push_device(
        &self,
        id: Uuid,
        user_id: Uuid,
        device_id: &str,
        platform: &str,
        provider: &str,
        token: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO push_devices (id, user_id, device_id, platform, provider, token, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
             ON CONFLICT(user_id, device_id) DO UPDATE SET
               platform=?4, provider=?5, token=?6, updated_at=?7",
            params![
                id.to_string(),
                user_id.to_string(),
                device_id,
                platform,
                provider,
                token,
                now
            ],
        )?;
        Ok(())
    }

    pub fn delete_push_device(&self, user_id: Uuid, device_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM push_devices WHERE user_id = ?1 AND device_id = ?2",
            params![user_id.to_string(), device_id],
        )?;
        Ok(())
    }

    pub fn list_push_tokens_for_user(&self, user_id: Uuid) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT token FROM push_devices WHERE user_id = ?1",
        )?;
        let rows = stmt.query_map(params![user_id.to_string()], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_session_member_user_ids(&self, session_id: Uuid) -> Result<Vec<Uuid>> {
        let mut stmt = self
            .conn
            .prepare("SELECT user_id FROM session_members WHERE session_id = ?1")?;
        let rows = stmt.query_map(params![session_id.to_string()], |row| {
            let id: String = row.get(0)?;
            Ok(Uuid::parse_str(&id).unwrap())
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_secret_refs(
        &self,
        session_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, String, Option<String>)>> {
        if let Some(sid) = session_id {
            let mut stmt = self.conn.prepare(
                "SELECT name, scope, ref_id, session_id FROM secret_refs
                 WHERE scope = 'system' OR scope = 'project' OR session_id = ?1
                 ORDER BY name",
            )?;
            let rows = stmt.query_map(params![sid.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT name, scope, ref_id, session_id FROM secret_refs ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn upsert_terminal(
        &self,
        id: Uuid,
        session_id: Uuid,
        name: &str,
        shell: &str,
        init_command: Option<&str>,
        cwd: &str,
        cols: u16,
        rows: u16,
        status: &str,
        tmux_target: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO terminals (id, session_id, name, shell, init_command, status, cols, rows, cwd, tmux_target, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name, shell=excluded.shell, init_command=excluded.init_command,
               status=excluded.status, cols=excluded.cols, rows=excluded.rows, cwd=excluded.cwd,
               tmux_target=excluded.tmux_target",
            params![
                id.to_string(),
                session_id.to_string(),
                name,
                shell,
                init_command,
                status,
                cols,
                rows,
                cwd,
                tmux_target,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_terminal(&self, id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM terminals WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_terminal_status(&self, id: Uuid, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE terminals SET status = ?1 WHERE id = ?2",
            params![status, id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_terminal_name(&self, id: Uuid, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE terminals SET name = ?1 WHERE id = ?2",
            params![name, id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_terminal(&self, id: Uuid) -> Result<Option<TerminalRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, name, shell, init_command, cwd, status, cols, rows, tmux_target
             FROM terminals WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_terminal_row(&row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_terminals_for_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TerminalRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, name, shell, init_command, cwd, status, cols, rows, tmux_target
             FROM terminals WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], map_terminal_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_terminals_with_status(&self, status: &str) -> Result<Vec<TerminalRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, name, shell, init_command, cwd, status, cols, rows, tmux_target
             FROM terminals WHERE status = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![status], map_terminal_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub type TerminalRow = (
    Uuid,
    Uuid,
    String,
    String,
    Option<String>,
    String,
    String,
    u16,
    u16,
    Option<String>,
);

fn map_terminal_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerminalRow> {
    Ok((
        Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
        Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
        Role::Agent => "agent",
    }
}
