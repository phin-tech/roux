//! SQLite-backed work item store — the sole `rusqlite` surface in roux-runtime.
//!
//! All DB access goes through `WorkItemStore`. Callers obtain a handle via
//! `WorkItemHandle` (see `work_item_service.rs`), which holds an
//! `Arc<Mutex<WorkItemStore>>` so the connection is shared without re-opening.
//!
//! Schema migrations use the `user_version` pragma (no extra dep).
//! `PRAGMA journal_mode=WAL` + `busy_timeout` are set at open time so
//! the desktop's idle reader and the daemon's writer can coexist.

use std::path::Path;

use rusqlite::{params, types::Type, Connection, OptionalExtension, Result as SqlResult};
use serde_json::Value;

use roux_core::{
    Attachment, AttachmentContentKind, AttachmentDocument, AttachmentInput, AttachmentTargetKind,
    ExternalRef, WorkItem, WorkItemDecision, WorkItemDecisionOption, WorkItemDecisionStatus,
    WorkItemInput, WorkItemRun, WorkItemRunEvent, WorkItemRunEventKind, WorkItemRunKind,
    WorkItemRunStatus, WorkItemStatus,
};

pub struct WorkItemStore {
    conn: Connection,
}

impl WorkItemStore {
    pub fn open(path: &Path) -> SqlResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(e.to_string()),
                )
            })?;
        }
        let conn = Connection::open(path)?;
        Self::configure_and_migrate(conn)
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_and_migrate(conn)
    }

    fn configure_and_migrate(conn: Connection) -> SqlResult<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;",
        )?;
        let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS work_items (
                    id          TEXT PRIMARY KEY,
                    project_id  TEXT,
                    parent_id   TEXT,
                    title       TEXT NOT NULL,
                    body        TEXT,
                    status      TEXT NOT NULL DEFAULT 'todo',
                    repo_path   TEXT,
                    agent_profile TEXT,
                    base_branch TEXT,
                    worktree_path TEXT,
                    branch      TEXT,
                    fetch_first INTEGER,
                    start_error TEXT,
                    session_id  TEXT,
                    provider    TEXT,
                    external_id TEXT,
                    external_url TEXT,
                    sort_order  REAL NOT NULL DEFAULT 0,
                    pinned_pr_url TEXT,
                    cost        REAL,
                    created_at  INTEGER NOT NULL,
                    updated_at  INTEGER NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS work_items_external
                    ON work_items(provider, external_id)
                    WHERE provider IS NOT NULL;
                CREATE INDEX IF NOT EXISTS work_items_project
                    ON work_items(project_id);
                CREATE INDEX IF NOT EXISTS work_items_status
                    ON work_items(status);
                PRAGMA user_version = 1;",
            )?;
            version = 1;
        }
        if version < 2 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS work_item_runs (
                    id            TEXT PRIMARY KEY,
                    work_item_id  TEXT NOT NULL,
                    kind          TEXT NOT NULL DEFAULT 'implementation',
                    session_id    TEXT,
                    provider      TEXT,
                    profile_id    TEXT,
                    status        TEXT NOT NULL DEFAULT 'running',
                    worktree_path TEXT,
                    branch        TEXT,
                    cost          REAL,
                    created_at    INTEGER NOT NULL,
                    started_at    INTEGER,
                    ended_at      INTEGER,
                    updated_at    INTEGER NOT NULL,
                    FOREIGN KEY(work_item_id) REFERENCES work_items(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS work_item_runs_item
                    ON work_item_runs(work_item_id, created_at);
                CREATE INDEX IF NOT EXISTS work_item_runs_status
                    ON work_item_runs(status);

                CREATE TABLE IF NOT EXISTS work_item_run_events (
                    id         TEXT PRIMARY KEY,
                    run_id     TEXT NOT NULL,
                    kind       TEXT NOT NULL,
                    payload    TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY(run_id) REFERENCES work_item_runs(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS work_item_run_events_run
                    ON work_item_run_events(run_id, created_at);

                CREATE TABLE IF NOT EXISTS work_item_decisions (
                    id             TEXT PRIMARY KEY,
                    run_id         TEXT NOT NULL,
                    question       TEXT NOT NULL,
                    options        TEXT NOT NULL,
                    default_value  TEXT,
                    status         TEXT NOT NULL DEFAULT 'pending',
                    resolved_value TEXT,
                    resolved_by    TEXT,
                    created_at     INTEGER NOT NULL,
                    resolved_at    INTEGER,
                    updated_at     INTEGER NOT NULL,
                    FOREIGN KEY(run_id) REFERENCES work_item_runs(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS work_item_decisions_run
                    ON work_item_decisions(run_id, created_at);
                CREATE INDEX IF NOT EXISTS work_item_decisions_status
                    ON work_item_decisions(status);
                PRAGMA user_version = 2;",
            )?;
            version = 2;
        }
        if version < 3 {
            conn.execute_batch(
                "ALTER TABLE work_item_decisions ADD COLUMN timeout_at INTEGER;
                CREATE INDEX IF NOT EXISTS work_item_decisions_timeout
                    ON work_item_decisions(timeout_at)
                    WHERE status = 'pending' AND timeout_at IS NOT NULL;
                PRAGMA user_version = 3;",
            )?;
            version = 3;
        }
        if (1..4).contains(&version) {
            add_column_if_missing(&conn, "work_items", "repo_path", "TEXT")?;
            add_column_if_missing(&conn, "work_items", "agent_profile", "TEXT")?;
            add_column_if_missing(&conn, "work_items", "base_branch", "TEXT")?;
            add_column_if_missing(&conn, "work_items", "worktree_path", "TEXT")?;
            add_column_if_missing(&conn, "work_items", "branch", "TEXT")?;
            add_column_if_missing(&conn, "work_items", "fetch_first", "INTEGER")?;
            add_column_if_missing(&conn, "work_items", "start_error", "TEXT")?;
            conn.execute_batch("PRAGMA user_version = 4;")?;
            version = 4;
        }
        if version < 5 {
            add_column_if_missing(
                &conn,
                "work_item_runs",
                "kind",
                "TEXT NOT NULL DEFAULT 'implementation'",
            )?;
            conn.execute_batch("PRAGMA user_version = 5;")?;
            version = 5;
        }
        if version < 6 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS attachments (
                    id           TEXT PRIMARY KEY,
                    target_kind  TEXT NOT NULL,
                    target_id    TEXT NOT NULL,
                    title        TEXT,
                    content_kind TEXT NOT NULL,
                    content      TEXT NOT NULL,
                    mime_type    TEXT,
                    source_path  TEXT,
                    byte_len     INTEGER NOT NULL,
                    sha256       TEXT NOT NULL,
                    created_at   INTEGER NOT NULL,
                    updated_at   INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS attachments_target
                    ON attachments(target_kind, target_id, created_at);
                PRAGMA user_version = 6;",
            )?;
            version = 6;
        }
        if version < 7 {
            add_column_if_missing(&conn, "work_item_runs", "pty_id", "TEXT")?;
            conn.execute_batch("PRAGMA user_version = 7;")?;
            version = 7;
        }
        debug_assert!(version >= 7);
        Ok(WorkItemStore { conn })
    }

    pub fn list(&self, project_id: Option<&str>) -> SqlResult<Vec<WorkItem>> {
        let sql = if project_id.is_some() {
            "SELECT id, project_id, parent_id, title, body, status, repo_path,
                    agent_profile, base_branch, worktree_path, branch, fetch_first,
                    start_error, session_id,
                    provider, external_id, external_url, sort_order, pinned_pr_url,
                    cost, created_at, updated_at
             FROM work_items
             WHERE project_id = ?1
             ORDER BY sort_order, created_at"
        } else {
            "SELECT id, project_id, parent_id, title, body, status, repo_path,
                    agent_profile, base_branch, worktree_path, branch, fetch_first,
                    start_error, session_id,
                    provider, external_id, external_url, sort_order, pinned_pr_url,
                    cost, created_at, updated_at
             FROM work_items
             ORDER BY sort_order, created_at"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(pid) = project_id {
            stmt.query_map(params![pid], row_to_work_item)?
        } else {
            stmt.query_map([], row_to_work_item)?
        };

        rows.collect()
    }

    pub fn get(&self, id: &str) -> SqlResult<Option<WorkItem>> {
        self.conn
            .query_row(
                "SELECT id, project_id, parent_id, title, body, status, repo_path,
                        agent_profile, base_branch, worktree_path, branch, fetch_first,
                        start_error, session_id,
                        provider, external_id, external_url, sort_order, pinned_pr_url,
                        cost, created_at, updated_at
                 FROM work_items WHERE id = ?1",
                params![id],
                row_to_work_item,
            )
            .optional()
    }

    pub fn create(&mut self, id: String, input: WorkItemInput, now: u64) -> SqlResult<WorkItem> {
        let status = input.status.as_ref().unwrap_or(&WorkItemStatus::Todo).as_str().to_string();
        let sort_order = input.sort_order.unwrap_or(0.0);
        let (provider, external_id, external_url) = split_external_ref(input.external_ref.as_ref());

        self.conn.execute(
            "INSERT INTO work_items
             (id, project_id, parent_id, title, body, status, repo_path,
              agent_profile, base_branch, worktree_path, branch, fetch_first,
              start_error, provider, external_id, external_url, sort_order,
              created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                id,
                input.project_id,
                input.parent_id,
                input.title,
                input.body,
                status,
                input.repo_path,
                input.agent_profile,
                input.base_branch,
                input.worktree_path,
                input.branch,
                input.fetch_first,
                input.start_error,
                provider,
                external_id,
                external_url,
                sort_order,
                now as i64,
                now as i64,
            ],
        )?;
        self.get(&id)?.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn update(
        &mut self,
        id: &str,
        input: WorkItemInput,
        now: u64,
    ) -> SqlResult<Option<WorkItem>> {
        let Some(existing) = self.get(id)? else {
            return Ok(None);
        };
        let status = input.status.as_ref().map(|s| s.as_str().to_string());
        let (provider, external_id, external_url) = split_external_ref(input.external_ref.as_ref());
        let repo_path_present = input.repo_path_present();
        let agent_profile_present = input.agent_profile_present();
        let base_branch_present = input.base_branch_present();
        let worktree_path_present = input.worktree_path_present();
        let branch_present = input.branch_present();
        let fetch_first_present = input.fetch_first_present();
        let update_start_error = input.start_error_present()
            || string_field_changed(
                repo_path_present,
                input.repo_path.as_deref(),
                existing.repo_path.as_deref(),
            )
            || string_field_changed(
                agent_profile_present,
                input.agent_profile.as_deref(),
                existing.agent_profile.as_deref(),
            )
            || string_field_changed(
                base_branch_present,
                input.base_branch.as_deref(),
                existing.base_branch.as_deref(),
            )
            || string_field_changed(
                worktree_path_present,
                input.worktree_path.as_deref(),
                existing.worktree_path.as_deref(),
            )
            || string_field_changed(
                branch_present,
                input.branch.as_deref(),
                existing.branch.as_deref(),
            )
            || option_field_changed(fetch_first_present, input.fetch_first, existing.fetch_first);
        self.conn.execute(
            "UPDATE work_items SET
                title       = ?2,
                body        = CASE WHEN ?20 THEN ?3 ELSE body END,
                status      = COALESCE(?4, status),
                repo_path   = CASE WHEN ?21 THEN ?5 ELSE repo_path END,
                agent_profile = CASE WHEN ?22 THEN ?6 ELSE agent_profile END,
                base_branch = CASE WHEN ?23 THEN ?7 ELSE base_branch END,
                worktree_path = CASE WHEN ?24 THEN ?8 ELSE worktree_path END,
                branch      = CASE WHEN ?25 THEN ?9 ELSE branch END,
                fetch_first = CASE WHEN ?26 THEN ?10 ELSE fetch_first END,
                start_error = CASE WHEN ?19 THEN ?11 ELSE start_error END,
                project_id  = CASE WHEN ?27 THEN ?12 ELSE project_id END,
                parent_id   = CASE WHEN ?28 THEN ?13 ELSE parent_id END,
                provider    = COALESCE(?14, provider),
                external_id = COALESCE(?15, external_id),
                external_url = COALESCE(?16, external_url),
                sort_order  = COALESCE(?17, sort_order),
                updated_at  = ?18
             WHERE id = ?1",
            params![
                id,
                input.title,
                input.body,
                status,
                input.repo_path,
                input.agent_profile,
                input.base_branch,
                input.worktree_path,
                input.branch,
                input.fetch_first,
                input.start_error,
                input.project_id,
                input.parent_id,
                provider,
                external_id,
                external_url,
                input.sort_order,
                now as i64,
                update_start_error,
                input.body_present(),
                repo_path_present,
                agent_profile_present,
                base_branch_present,
                worktree_path_present,
                branch_present,
                fetch_first_present,
                input.project_id_present(),
                input.parent_id_present(),
            ],
        )?;
        self.get(id)
    }

    pub fn move_item(
        &mut self,
        id: &str,
        status: WorkItemStatus,
        sort_order: f64,
        now: u64,
    ) -> SqlResult<Option<WorkItem>> {
        self.conn.execute(
            "UPDATE work_items SET status = ?2, sort_order = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status.as_str(), sort_order, now as i64],
        )?;
        self.get(id)
    }

    pub fn delete(&mut self, id: &str) -> SqlResult<bool> {
        let changed = self.conn.execute("DELETE FROM work_items WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    pub fn create_attachment(
        &mut self,
        id: String,
        input: AttachmentInput,
        byte_len: u64,
        sha256: String,
        now: u64,
    ) -> SqlResult<Attachment> {
        self.conn.execute(
            "INSERT INTO attachments
             (id, target_kind, target_id, title, content_kind, content, mime_type, source_path,
              byte_len, sha256, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                input.target_kind.as_str(),
                input.target_id,
                input.title,
                input.content_kind.as_str(),
                input.content,
                input.mime_type,
                input.source_path,
                byte_len as i64,
                sha256,
                now as i64,
                now as i64,
            ],
        )?;
        self.get_attachment(&id)?.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn get_attachment(&self, id: &str) -> SqlResult<Option<Attachment>> {
        self.conn
            .query_row(
                "SELECT id, target_kind, target_id, title, content_kind, mime_type, source_path,
                        byte_len, sha256, created_at, updated_at
                 FROM attachments
                 WHERE id = ?1",
                params![id],
                row_to_attachment,
            )
            .optional()
    }

    pub fn list_attachments(
        &self,
        target_kind: Option<AttachmentTargetKind>,
        target_id: Option<&str>,
    ) -> SqlResult<Vec<Attachment>> {
        match (target_kind, target_id) {
            (Some(kind), Some(target_id)) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, target_kind, target_id, title, content_kind, mime_type,
                            source_path, byte_len, sha256, created_at, updated_at
                     FROM attachments
                     WHERE target_kind = ?1 AND target_id = ?2
                     ORDER BY created_at, rowid",
                )?;
                let rows = stmt.query_map(params![kind.as_str(), target_id], row_to_attachment)?;
                rows.collect()
            }
            (Some(kind), None) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, target_kind, target_id, title, content_kind, mime_type,
                            source_path, byte_len, sha256, created_at, updated_at
                     FROM attachments
                     WHERE target_kind = ?1
                     ORDER BY created_at, rowid",
                )?;
                let rows = stmt.query_map(params![kind.as_str()], row_to_attachment)?;
                rows.collect()
            }
            (None, Some(target_id)) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, target_kind, target_id, title, content_kind, mime_type,
                            source_path, byte_len, sha256, created_at, updated_at
                     FROM attachments
                     WHERE target_id = ?1
                     ORDER BY created_at, rowid",
                )?;
                let rows = stmt.query_map(params![target_id], row_to_attachment)?;
                rows.collect()
            }
            (None, None) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, target_kind, target_id, title, content_kind, mime_type,
                            source_path, byte_len, sha256, created_at, updated_at
                     FROM attachments
                     ORDER BY created_at, rowid",
                )?;
                let rows = stmt.query_map([], row_to_attachment)?;
                rows.collect()
            }
        }
    }

    pub fn get_attachment_document(
        &self,
        document_id: &str,
    ) -> SqlResult<Option<AttachmentDocument>> {
        let Some(lookup) = parse_document_lookup(document_id) else {
            return Ok(None);
        };
        let (target_id, attachment_id) = lookup;
        let sql = if target_id.is_some() {
            "SELECT id, target_kind, target_id, title, content_kind, mime_type, source_path,
                    byte_len, sha256, created_at, updated_at, content
             FROM attachments
             WHERE target_id = ?1 AND id = ?2"
        } else {
            "SELECT id, target_kind, target_id, title, content_kind, mime_type, source_path,
                    byte_len, sha256, created_at, updated_at, content
             FROM attachments
             WHERE id = ?1"
        };
        if let Some(target_id) = target_id {
            self.conn
                .query_row(sql, params![target_id, attachment_id], row_to_attachment_document)
                .optional()
        } else {
            self.conn.query_row(sql, params![attachment_id], row_to_attachment_document).optional()
        }
    }

    pub fn set_session(
        &mut self,
        id: &str,
        session_id: &str,
        now: u64,
    ) -> SqlResult<Option<WorkItem>> {
        self.ensure_session_unbound_or_self(id, session_id)?;
        self.conn.execute(
            "UPDATE work_items SET session_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, session_id, now as i64],
        )?;
        self.get(id)
    }

    pub fn detach_session(&mut self, id: &str, now: u64) -> SqlResult<Option<WorkItem>> {
        self.conn.execute(
            "UPDATE work_items SET session_id = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now as i64],
        )?;
        self.get(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_start_failure(
        &mut self,
        id: &str,
        error: &str,
        session_id: Option<&str>,
        worktree_path: Option<&str>,
        agent_profile: Option<&str>,
        repo_path: Option<&str>,
        base_branch: Option<&str>,
        now: u64,
    ) -> SqlResult<Option<WorkItem>> {
        self.conn.execute(
            "UPDATE work_items SET
                start_error = ?2,
                session_id = COALESCE(?3, session_id),
                worktree_path = COALESCE(?4, worktree_path),
                agent_profile = COALESCE(?5, agent_profile),
                repo_path = COALESCE(?6, repo_path),
                base_branch = COALESCE(?7, base_branch),
                updated_at = ?8
             WHERE id = ?1",
            params![
                id,
                error,
                session_id,
                worktree_path,
                agent_profile,
                repo_path,
                base_branch,
                now as i64,
            ],
        )?;
        self.get(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_start(
        &mut self,
        id: &str,
        session_id: &str,
        worktree_path: Option<&str>,
        agent_profile: Option<&str>,
        repo_path: Option<&str>,
        base_branch: Option<&str>,
        sort_order: f64,
        now: u64,
    ) -> SqlResult<Option<WorkItem>> {
        self.ensure_session_unbound_or_self(id, session_id)?;
        self.conn.execute(
            "UPDATE work_items SET
                session_id = ?2,
                status = ?3,
                sort_order = ?4,
                worktree_path = COALESCE(?5, worktree_path),
                agent_profile = COALESCE(?6, agent_profile),
                repo_path = COALESCE(?7, repo_path),
                base_branch = COALESCE(?8, base_branch),
                start_error = NULL,
                updated_at = ?9
             WHERE id = ?1",
            params![
                id,
                session_id,
                WorkItemStatus::Doing.as_str(),
                sort_order,
                worktree_path,
                agent_profile,
                repo_path,
                base_branch,
                now as i64,
            ],
        )?;
        self.get(id)
    }

    /// Bind a session only if the item still exists and is unbound. Returns
    /// `true` if this call performed the bind, `false` if the item was already
    /// bound (lost a dispatch race) or no longer exists. The `session_id IS
    /// NULL` guard makes concurrent dispatches mutually exclusive at the DB.
    pub fn set_session_if_unbound(
        &mut self,
        id: &str,
        session_id: &str,
        now: u64,
    ) -> SqlResult<bool> {
        self.ensure_session_unbound_or_self(id, session_id)?;
        let changed = self.conn.execute(
            "UPDATE work_items SET session_id = ?2, updated_at = ?3
             WHERE id = ?1 AND session_id IS NULL",
            params![id, session_id, now as i64],
        )?;
        Ok(changed > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_run(
        &mut self,
        run_id: String,
        work_item_id: &str,
        session_id: &str,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
        sort_order: f64,
        now: u64,
    ) -> SqlResult<Option<(WorkItem, WorkItemRun)>> {
        self.ensure_session_unbound_or_self(work_item_id, session_id)?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE work_items
             SET session_id = ?2, status = ?3, sort_order = ?4, updated_at = ?5
             WHERE id = ?1 AND session_id IS NULL",
            params![
                work_item_id,
                session_id,
                WorkItemStatus::Doing.as_str(),
                sort_order,
                now as i64,
            ],
        )?;
        if changed == 0 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO work_item_runs
             (id, work_item_id, kind, session_id, provider, profile_id, status,
              worktree_path, branch, created_at, started_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                run_id,
                work_item_id,
                WorkItemRunKind::Implementation.as_str(),
                session_id,
                provider,
                profile_id,
                WorkItemRunStatus::Running.as_str(),
                worktree_path,
                branch,
                now as i64,
                now as i64,
                now as i64,
            ],
        )?;
        tx.commit()?;

        let item = self.get(work_item_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let run = self.get_run(&run_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        Ok(Some((item, run)))
    }

    fn ensure_session_unbound_or_self(&self, id: &str, session_id: &str) -> SqlResult<()> {
        if let Some(bound_id) = self.session_bound_work_item_id(session_id, Some(id))? {
            return Err(session_already_bound_error(session_id, &bound_id));
        }
        Ok(())
    }

    pub fn session_bound_work_item_id(
        &self,
        session_id: &str,
        except_id: Option<&str>,
    ) -> SqlResult<Option<String>> {
        match except_id {
            Some(id) => self
                .conn
                .query_row(
                    "SELECT id FROM work_items
                     WHERE session_id = ?1 AND id != ?2
                     LIMIT 1",
                    params![session_id, id],
                    |row| row.get(0),
                )
                .optional(),
            None => self
                .conn
                .query_row(
                    "SELECT id FROM work_items
                     WHERE session_id = ?1
                     LIMIT 1",
                    params![session_id],
                    |row| row.get(0),
                )
                .optional(),
        }
    }

    pub fn has_active_run(&self, work_item_id: &str) -> SqlResult<bool> {
        self.conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM work_item_runs
                WHERE work_item_id = ?1
                  AND status NOT IN (?2, ?3, ?4)
            )",
            params![
                work_item_id,
                WorkItemRunStatus::Done.as_str(),
                WorkItemRunStatus::Failed.as_str(),
                WorkItemRunStatus::Stopped.as_str(),
            ],
            |row| row.get(0),
        )
    }

    /// Upsert by `(provider, external_id)`: insert if no match, otherwise
    /// update `title`, `body`, `status`, and `updated_at`.
    pub fn upsert_by_external(
        &mut self,
        id: String,
        input: WorkItemInput,
        now: u64,
    ) -> SqlResult<WorkItem> {
        let (provider, external_id, _) = split_external_ref(input.external_ref.as_ref());

        // Try to find existing by (provider, external_id).
        let existing_id: Option<String> = if provider.is_some() && external_id.is_some() {
            self.conn
                .query_row(
                    "SELECT id FROM work_items WHERE provider = ?1 AND external_id = ?2",
                    params![provider, external_id],
                    |row| row.get(0),
                )
                .optional()?
        } else {
            None
        };

        if let Some(eid) = existing_id {
            // Update existing item: title, body, status, updated_at.
            let status = input.status.as_ref().map(|s| s.as_str().to_string());
            self.conn.execute(
                "UPDATE work_items SET
                    title      = ?2,
                    body       = COALESCE(?3, body),
                    status     = COALESCE(?4, status),
                    updated_at = ?5
                 WHERE id = ?1",
                params![eid, input.title, input.body, status, now as i64],
            )?;
            self.get(&eid)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
        } else {
            self.create(id, input, now)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_run(
        &mut self,
        id: String,
        work_item_id: &str,
        session_id: Option<&str>,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
        now: u64,
    ) -> SqlResult<WorkItemRun> {
        self.create_run_with_status(
            id,
            work_item_id,
            session_id,
            provider,
            profile_id,
            worktree_path,
            branch,
            WorkItemRunStatus::Running,
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_run_with_status(
        &mut self,
        id: String,
        work_item_id: &str,
        session_id: Option<&str>,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
        status: WorkItemRunStatus,
        now: u64,
    ) -> SqlResult<WorkItemRun> {
        self.create_run_with_kind_status(
            id,
            work_item_id,
            WorkItemRunKind::Implementation,
            session_id,
            provider,
            profile_id,
            worktree_path,
            branch,
            status,
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_run_with_kind_status(
        &mut self,
        id: String,
        work_item_id: &str,
        kind: WorkItemRunKind,
        session_id: Option<&str>,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
        status: WorkItemRunStatus,
        now: u64,
    ) -> SqlResult<WorkItemRun> {
        let is_terminal = matches!(
            status,
            WorkItemRunStatus::Failed | WorkItemRunStatus::Stopped | WorkItemRunStatus::Done
        );
        let started_at = (status == WorkItemRunStatus::Running).then_some(now as i64);
        let tx = self.conn.transaction()?;
        if !is_terminal {
            let active: bool = tx.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM work_item_runs
                    WHERE work_item_id = ?1
                      AND status NOT IN (?2, ?3, ?4)
                      AND (
                          ?5 IS NULL
                          OR session_id IS NULL
                          OR session_id != ?5
                      )
                )",
                params![
                    work_item_id,
                    WorkItemRunStatus::Done.as_str(),
                    WorkItemRunStatus::Failed.as_str(),
                    WorkItemRunStatus::Stopped.as_str(),
                    session_id,
                ],
                |row| row.get(0),
            )?;
            if active {
                tx.rollback()?;
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some("active work item run already exists".into()),
                ));
            }
        }
        tx.execute(
            "INSERT INTO work_item_runs
             (id, work_item_id, kind, session_id, provider, profile_id, status,
              worktree_path, branch, created_at, started_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                work_item_id,
                kind.as_str(),
                session_id,
                provider,
                profile_id,
                status.as_str(),
                worktree_path,
                branch,
                now as i64,
                started_at,
                now as i64,
            ],
        )?;
        tx.commit()?;
        self.get_run(&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn get_run(&self, id: &str) -> SqlResult<Option<WorkItemRun>> {
        self.conn
            .query_row(
                "SELECT id, work_item_id, kind, session_id, pty_id, provider, profile_id, status,
                        worktree_path, branch, cost, created_at, started_at, ended_at, updated_at
                 FROM work_item_runs
                 WHERE id = ?1",
                params![id],
                row_to_work_item_run,
            )
            .optional()
    }

    pub fn list_runs(&self, work_item_id: Option<&str>) -> SqlResult<Vec<WorkItemRun>> {
        let sql = if work_item_id.is_some() {
            "SELECT id, work_item_id, kind, session_id, pty_id, provider, profile_id, status,
                    worktree_path, branch, cost, created_at, started_at, ended_at, updated_at
             FROM work_item_runs
             WHERE work_item_id = ?1
             ORDER BY rowid"
        } else {
            "SELECT id, work_item_id, kind, session_id, pty_id, provider, profile_id, status,
                    worktree_path, branch, cost, created_at, started_at, ended_at, updated_at
             FROM work_item_runs
             ORDER BY rowid"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(work_item_id) = work_item_id {
            stmt.query_map(params![work_item_id], row_to_work_item_run)?
        } else {
            stmt.query_map([], row_to_work_item_run)?
        };
        rows.collect()
    }

    pub fn append_run_event(
        &mut self,
        id: String,
        run_id: &str,
        kind: WorkItemRunEventKind,
        payload: Value,
        now: u64,
    ) -> SqlResult<WorkItemRunEvent> {
        let payload = serde_json::to_string(&payload)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.conn.execute(
            "INSERT INTO work_item_run_events (id, run_id, kind, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, run_id, kind.as_str(), payload, now as i64],
        )?;
        self.get_run_event(&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn set_run_pty_id(
        &mut self,
        id: &str,
        pty_id: Option<&str>,
        now: u64,
    ) -> SqlResult<Option<WorkItemRun>> {
        let changed = self.conn.execute(
            "UPDATE work_item_runs
             SET pty_id = ?2, updated_at = ?3
             WHERE id = ?1",
            params![id, pty_id, now as i64],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_run(id)
    }

    pub fn get_run_event(&self, id: &str) -> SqlResult<Option<WorkItemRunEvent>> {
        self.conn
            .query_row(
                "SELECT id, run_id, kind, payload, created_at
                 FROM work_item_run_events
                 WHERE id = ?1",
                params![id],
                row_to_work_item_run_event,
            )
            .optional()
    }

    pub fn list_run_events(&self, run_id: &str) -> SqlResult<Vec<WorkItemRunEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, kind, payload, created_at
             FROM work_item_run_events
             WHERE run_id = ?1
             ORDER BY rowid",
        )?;
        let events = stmt.query_map(params![run_id], row_to_work_item_run_event)?.collect();
        events
    }

    pub fn update_run_status_with_event(
        &mut self,
        id: &str,
        status: WorkItemRunStatus,
        event_id: String,
        payload: Value,
        now: u64,
    ) -> SqlResult<Option<(WorkItemRun, WorkItemRunEvent)>> {
        if self.get_run(id)?.is_none() {
            return Ok(None);
        }
        let payload = serde_json::to_string(&payload)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let is_terminal = matches!(
            status,
            WorkItemRunStatus::Review
                | WorkItemRunStatus::Failed
                | WorkItemRunStatus::Stopped
                | WorkItemRunStatus::Done
        );
        let is_running = status == WorkItemRunStatus::Running;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE work_item_runs
             SET status = ?2,
                 updated_at = ?3,
                 ended_at = CASE WHEN ?4 THEN COALESCE(ended_at, ?3) ELSE ended_at END,
                 started_at = CASE WHEN ?9 THEN COALESCE(started_at, ?3) ELSE started_at END
             WHERE id = ?1
               AND status NOT IN (?5, ?6, ?7, ?8)",
            params![
                id,
                status.as_str(),
                now as i64,
                is_terminal,
                WorkItemRunStatus::Review.as_str(),
                WorkItemRunStatus::Done.as_str(),
                WorkItemRunStatus::Failed.as_str(),
                WorkItemRunStatus::Stopped.as_str(),
                is_running,
            ],
        )?;
        if changed == 0 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO work_item_run_events (id, run_id, kind, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event_id,
                id,
                WorkItemRunEventKind::StatusChanged.as_str(),
                payload,
                now as i64,
            ],
        )?;
        tx.commit()?;

        let run = self.get_run(id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let event = self.get_run_event(&event_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        Ok(Some((run, event)))
    }

    pub fn accept_review(
        &mut self,
        work_item_id: &str,
        event_id: String,
        payload: Value,
        now: u64,
    ) -> SqlResult<Option<(WorkItem, WorkItemRun, WorkItemRunEvent)>> {
        if self.get(work_item_id)?.is_none() {
            return Ok(None);
        }
        let payload = serde_json::to_string(&payload)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let tx = self.conn.transaction()?;
        let run_id = tx
            .query_row(
                "SELECT id
                 FROM work_item_runs
                 WHERE work_item_id = ?1
                   AND kind = ?2
                   AND status = ?3
                 ORDER BY updated_at DESC, rowid DESC
                 LIMIT 1",
                params![
                    work_item_id,
                    WorkItemRunKind::Implementation.as_str(),
                    WorkItemRunStatus::Review.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(run_id) = run_id else {
            tx.rollback()?;
            return Ok(None);
        };
        let changed = tx.execute(
            "UPDATE work_item_runs
             SET status = ?2,
                 updated_at = ?3,
                 ended_at = COALESCE(ended_at, ?3)
             WHERE id = ?1 AND status = ?4",
            params![
                run_id,
                WorkItemRunStatus::Done.as_str(),
                now as i64,
                WorkItemRunStatus::Review.as_str(),
            ],
        )?;
        if changed == 0 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "UPDATE work_items
             SET status = ?2, updated_at = ?3
             WHERE id = ?1",
            params![work_item_id, WorkItemStatus::Done.as_str(), now as i64],
        )?;
        tx.execute(
            "INSERT INTO work_item_run_events (id, run_id, kind, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event_id,
                run_id,
                WorkItemRunEventKind::StatusChanged.as_str(),
                payload,
                now as i64,
            ],
        )?;
        tx.commit()?;

        let item = self.get(work_item_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let run = self.get_run(&run_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let event = self.get_run_event(&event_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        Ok(Some((item, run, event)))
    }

    pub fn request_review(
        &mut self,
        run_id: &str,
        event_id: String,
        payload: Value,
        now: u64,
    ) -> SqlResult<Option<(WorkItem, WorkItemRun, WorkItemRunEvent)>> {
        let payload = serde_json::to_string(&payload)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let tx = self.conn.transaction()?;
        let run_row = tx
            .query_row(
                "SELECT work_item_id, kind
                 FROM work_item_runs
                 WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((work_item_id, kind)) = run_row else {
            tx.rollback()?;
            return Ok(None);
        };
        if kind != WorkItemRunKind::Implementation.as_str() {
            tx.rollback()?;
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                Some("work item review can only be requested for implementation runs".into()),
            ));
        }
        let changed = tx.execute(
            "UPDATE work_item_runs
             SET status = ?2,
                 updated_at = ?3,
                 ended_at = COALESCE(ended_at, ?3)
             WHERE id = ?1
               AND status NOT IN (?4, ?5, ?6)",
            params![
                run_id,
                WorkItemRunStatus::Review.as_str(),
                now as i64,
                WorkItemRunStatus::Done.as_str(),
                WorkItemRunStatus::Failed.as_str(),
                WorkItemRunStatus::Stopped.as_str(),
            ],
        )?;
        if changed == 0 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "UPDATE work_items
             SET status = ?2, updated_at = ?3
             WHERE id = ?1",
            params![work_item_id, WorkItemStatus::Review.as_str(), now as i64],
        )?;
        tx.execute(
            "INSERT INTO work_item_run_events (id, run_id, kind, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event_id,
                run_id,
                WorkItemRunEventKind::StatusChanged.as_str(),
                payload,
                now as i64,
            ],
        )?;
        tx.commit()?;

        let item = self.get(&work_item_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let run = self.get_run(run_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let event = self.get_run_event(&event_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        Ok(Some((item, run, event)))
    }

    pub fn create_decision(
        &mut self,
        id: String,
        run_id: &str,
        question: &str,
        options: Vec<WorkItemDecisionOption>,
        default_value: Option<&str>,
        timeout_at: Option<u64>,
        now: u64,
    ) -> SqlResult<WorkItemDecision> {
        let options_json = serde_json::to_string(&options)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE work_item_runs
             SET status = ?2, updated_at = ?3
             WHERE id = ?1
               AND status NOT IN (?4, ?5, ?6)",
            params![
                run_id,
                WorkItemRunStatus::Blocked.as_str(),
                now as i64,
                WorkItemRunStatus::Done.as_str(),
                WorkItemRunStatus::Failed.as_str(),
                WorkItemRunStatus::Stopped.as_str(),
            ],
        )?;
        if changed == 0 {
            tx.rollback()?;
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.execute(
            "INSERT INTO work_item_decisions
             (id, run_id, question, options, default_value, timeout_at, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                run_id,
                question,
                options_json,
                default_value,
                timeout_at.map(|value| value as i64),
                WorkItemDecisionStatus::Pending.as_str(),
                now as i64,
                now as i64,
            ],
        )?;
        tx.commit()?;
        self.get_decision(&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn get_decision(&self, id: &str) -> SqlResult<Option<WorkItemDecision>> {
        self.conn
            .query_row(
                "SELECT id, run_id, question, options, default_value, timeout_at, status,
                        resolved_value, resolved_by, created_at, resolved_at, updated_at
                 FROM work_item_decisions
                 WHERE id = ?1",
                params![id],
                row_to_work_item_decision,
            )
            .optional()
    }

    pub fn list_pending_decisions(
        &self,
        work_item_id: Option<&str>,
    ) -> SqlResult<Vec<WorkItemDecision>> {
        let sql = if work_item_id.is_some() {
            "SELECT d.id, d.run_id, d.question, d.options, d.default_value, d.timeout_at, d.status,
                    d.resolved_value, d.resolved_by, d.created_at, d.resolved_at, d.updated_at
             FROM work_item_decisions d
             JOIN work_item_runs r ON r.id = d.run_id
             WHERE d.status = ?1 AND r.work_item_id = ?2
             ORDER BY d.created_at, d.id"
        } else {
            "SELECT id, run_id, question, options, default_value, timeout_at, status,
                    resolved_value, resolved_by, created_at, resolved_at, updated_at
             FROM work_item_decisions
             WHERE status = ?1
             ORDER BY created_at, id"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(work_item_id) = work_item_id {
            stmt.query_map(
                params![WorkItemDecisionStatus::Pending.as_str(), work_item_id],
                row_to_work_item_decision,
            )?
        } else {
            stmt.query_map(
                params![WorkItemDecisionStatus::Pending.as_str()],
                row_to_work_item_decision,
            )?
        };
        rows.collect()
    }

    pub fn resolve_decision(
        &mut self,
        id: &str,
        value: &str,
        resolved_by: Option<&str>,
        now: u64,
    ) -> SqlResult<Option<WorkItemDecision>> {
        let Some(existing) = self.get_decision(id)? else {
            return Ok(None);
        };
        if existing.status != WorkItemDecisionStatus::Pending {
            return Ok(None);
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE work_item_decisions SET
                status = ?2,
                resolved_value = ?3,
                resolved_by = ?4,
                resolved_at = ?5,
                updated_at = ?6
             WHERE id = ?1",
            params![
                id,
                WorkItemDecisionStatus::Resolved.as_str(),
                value,
                resolved_by,
                now as i64,
                now as i64,
            ],
        )?;
        tx.execute(
            "UPDATE work_item_runs
             SET status = ?2, updated_at = ?3
             WHERE id = ?1 AND status = ?4
               AND NOT EXISTS (
                   SELECT 1 FROM work_item_decisions
                   WHERE run_id = ?1 AND status = ?5
               )",
            params![
                existing.run_id,
                WorkItemRunStatus::Running.as_str(),
                now as i64,
                WorkItemRunStatus::Blocked.as_str(),
                WorkItemDecisionStatus::Pending.as_str(),
            ],
        )?;
        tx.commit()?;
        self.get_decision(id)
    }

    pub fn timeout_due_decisions(&mut self, now: u64) -> SqlResult<Vec<WorkItemDecision>> {
        let ids = {
            let mut stmt = self.conn.prepare(
                "SELECT id
                 FROM work_item_decisions
                 WHERE status = ?1
                   AND timeout_at IS NOT NULL
                   AND timeout_at <= ?2
                   AND default_value IS NOT NULL
                 ORDER BY timeout_at, created_at, id",
            )?;
            let rows = stmt.query_map(
                params![WorkItemDecisionStatus::Pending.as_str(), now as i64],
                |row| row.get::<_, String>(0),
            )?;
            rows.collect::<SqlResult<Vec<_>>>()?
        };

        let mut decisions = Vec::new();
        for id in ids {
            if let Some(decision) = self.timeout_decision_to_default(&id, now)? {
                decisions.push(decision);
            }
        }
        Ok(decisions)
    }

    pub fn timeout_decision_to_default(
        &mut self,
        id: &str,
        now: u64,
    ) -> SqlResult<Option<WorkItemDecision>> {
        let Some(existing) = self.get_decision(id)? else {
            return Ok(None);
        };
        if existing.status != WorkItemDecisionStatus::Pending {
            return Ok(None);
        }
        let Some(default_value) = existing.default_value.clone() else {
            return Ok(None);
        };
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE work_item_decisions SET
                status = ?2,
                resolved_value = ?3,
                resolved_by = ?4,
                resolved_at = ?5,
                updated_at = ?6
             WHERE id = ?1 AND status = ?7",
            params![
                id,
                WorkItemDecisionStatus::TimedOut.as_str(),
                default_value,
                "timeout",
                now as i64,
                now as i64,
                WorkItemDecisionStatus::Pending.as_str(),
            ],
        )?;
        tx.execute(
            "UPDATE work_item_runs
             SET status = ?2, updated_at = ?3
             WHERE id = ?1 AND status = ?4
               AND NOT EXISTS (
                   SELECT 1 FROM work_item_decisions
                   WHERE run_id = ?1 AND status = ?5
               )",
            params![
                existing.run_id,
                WorkItemRunStatus::Running.as_str(),
                now as i64,
                WorkItemRunStatus::Blocked.as_str(),
                WorkItemDecisionStatus::Pending.as_str(),
            ],
        )?;
        tx.commit()?;
        self.get_decision(id)
    }
}

fn split_external_ref(r: Option<&ExternalRef>) -> (Option<String>, Option<String>, Option<String>) {
    match r {
        Some(r) => (Some(r.provider.clone()), Some(r.external_id.clone()), r.url.clone()),
        None => (None, None, None),
    }
}

fn string_field_changed(present: bool, next: Option<&str>, current: Option<&str>) -> bool {
    present && next != current
}

fn option_field_changed<T: PartialEq>(present: bool, next: Option<T>, current: Option<T>) -> bool {
    present && next != current
}

fn session_already_bound_error(session_id: &str, work_item_id: &str) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        Some(format!("session already bound to work item {work_item_id}: {session_id}")),
    )
}

fn row_to_work_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItem> {
    let status_str: String = row.get(5)?;
    let status = WorkItemStatus::from_str_opt(&status_str).unwrap_or_default();
    Ok(WorkItem {
        id: row.get(0)?,
        project_id: row.get(1)?,
        parent_id: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        status,
        repo_path: row.get(6)?,
        agent_profile: row.get(7)?,
        base_branch: row.get(8)?,
        worktree_path: row.get(9)?,
        branch: row.get(10)?,
        fetch_first: row.get(11)?,
        start_error: row.get(12)?,
        session_id: row.get(13)?,
        provider: row.get(14)?,
        external_id: row.get(15)?,
        external_url: row.get(16)?,
        sort_order: row.get(17)?,
        pinned_pr_url: row.get(18)?,
        cost: row.get(19)?,
        created_at: row.get::<_, i64>(20)? as u64,
        updated_at: row.get::<_, i64>(21)? as u64,
    })
}

fn row_to_work_item_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItemRun> {
    let kind_str: String = row.get(2)?;
    let kind = WorkItemRunKind::from_str_opt(&kind_str).unwrap_or_default();
    let status_str: String = row.get(7)?;
    let status = WorkItemRunStatus::from_str_opt(&status_str).unwrap_or_default();
    Ok(WorkItemRun {
        id: row.get(0)?,
        work_item_id: row.get(1)?,
        kind,
        session_id: row.get(3)?,
        pty_id: row.get(4)?,
        provider: row.get(5)?,
        profile_id: row.get(6)?,
        status,
        worktree_path: row.get(8)?,
        branch: row.get(9)?,
        cost: row.get(10)?,
        created_at: row.get::<_, i64>(11)? as u64,
        started_at: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
        ended_at: row.get::<_, Option<i64>>(13)?.map(|value| value as u64),
        updated_at: row.get::<_, i64>(14)? as u64,
    })
}

fn row_to_work_item_run_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItemRunEvent> {
    let kind_str: String = row.get(2)?;
    let kind = WorkItemRunEventKind::from_str_opt(&kind_str).unwrap_or(WorkItemRunEventKind::Text);
    let payload_str: String = row.get(3)?;
    let payload = serde_json::from_str(&payload_str).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(WorkItemRunEvent {
        id: row.get(0)?,
        run_id: row.get(1)?,
        kind,
        payload,
        created_at: row.get::<_, i64>(4)? as u64,
    })
}

fn row_to_work_item_decision(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItemDecision> {
    let options_str: String = row.get(3)?;
    let options = serde_json::from_str(&options_str).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let status_str: String = row.get(6)?;
    let status = WorkItemDecisionStatus::from_str_opt(&status_str).unwrap_or_default();
    Ok(WorkItemDecision {
        id: row.get(0)?,
        run_id: row.get(1)?,
        question: row.get(2)?,
        options,
        default_value: row.get(4)?,
        timeout_at: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
        status,
        resolved_value: row.get(7)?,
        resolved_by: row.get(8)?,
        created_at: row.get::<_, i64>(9)? as u64,
        resolved_at: row.get::<_, Option<i64>>(10)?.map(|value| value as u64),
        updated_at: row.get::<_, i64>(11)? as u64,
    })
}

fn row_to_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    let target_kind_str: String = row.get(1)?;
    let target_kind = AttachmentTargetKind::from_str_opt(&target_kind_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            Type::Text,
            format!("unknown attachment target kind: {target_kind_str}").into(),
        )
    })?;
    let content_kind_str: String = row.get(4)?;
    let content_kind = AttachmentContentKind::from_str_opt(&content_kind_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            Type::Text,
            format!("unknown attachment content kind: {content_kind_str}").into(),
        )
    })?;
    let id: String = row.get(0)?;
    let target_id: String = row.get(2)?;
    Ok(Attachment {
        document_id: attachment_document_id(&target_id, &id),
        id,
        target_kind,
        target_id,
        title: row.get(3)?,
        content_kind,
        mime_type: row.get(5)?,
        source_path: row.get(6)?,
        byte_len: row.get::<_, i64>(7)? as u64,
        sha256: row.get(8)?,
        created_at: row.get::<_, i64>(9)? as u64,
        updated_at: row.get::<_, i64>(10)? as u64,
    })
}

fn row_to_attachment_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentDocument> {
    Ok(AttachmentDocument { attachment: row_to_attachment(row)?, content: row.get(11)? })
}

fn attachment_document_id(target_id: &str, attachment_id: &str) -> String {
    format!("{target_id}.{attachment_id}")
}

fn parse_document_lookup(raw: &str) -> Option<(Option<&str>, &str)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let trimmed = trimmed
        .strip_prefix("session/")
        .or_else(|| trimmed.strip_prefix("work-item/"))
        .or_else(|| trimmed.strip_prefix("workItem/"))
        .unwrap_or(trimmed);
    match trimmed.rsplit_once('.') {
        Some((target_id, attachment_id)) if !target_id.is_empty() && !attachment_id.is_empty() => {
            Some((Some(target_id), attachment_id))
        }
        _ => Some((None, trimmed)),
    }
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> SqlResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> SqlResult<()> {
    if !table_has_column(conn, table, column)? {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition};"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::WorkItemInput;

    fn input(title: &str) -> WorkItemInput {
        WorkItemInput { title: title.to_string(), ..Default::default() }
    }

    fn input_with_ref(title: &str, provider: &str, ext_id: &str) -> WorkItemInput {
        WorkItemInput {
            title: title.to_string(),
            external_ref: Some(ExternalRef {
                provider: provider.to_string(),
                external_id: ext_id.to_string(),
                url: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn migration_sets_user_version_to_current() {
        let store = WorkItemStore::open_in_memory().unwrap();
        let version: i64 =
            store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 7);
    }

    #[test]
    fn fresh_database_reopens_without_duplicate_v4_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.db");
        {
            let store = WorkItemStore::open(&path).unwrap();
            let version: i64 =
                store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
            assert_eq!(version, 7);
            assert!(table_has_column(&store.conn, "work_items", "repo_path").unwrap());
            assert!(table_has_column(&store.conn, "work_items", "branch").unwrap());
            assert!(table_has_column(&store.conn, "work_items", "fetch_first").unwrap());
            assert!(table_has_column(&store.conn, "work_item_runs", "kind").unwrap());
            assert!(table_has_column(&store.conn, "work_item_runs", "pty_id").unwrap());
        }

        let store = WorkItemStore::open(&path).unwrap();
        let version: i64 =
            store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 7);
        assert!(table_has_column(&store.conn, "work_items", "repo_path").unwrap());
        assert!(table_has_column(&store.conn, "work_items", "branch").unwrap());
        assert!(table_has_column(&store.conn, "work_items", "fetch_first").unwrap());
        assert!(table_has_column(&store.conn, "work_item_runs", "kind").unwrap());
        assert!(table_has_column(&store.conn, "work_item_runs", "pty_id").unwrap());
    }

    #[test]
    fn v4_migration_repairs_partially_added_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE work_items (
                    id          TEXT PRIMARY KEY,
                    project_id  TEXT,
                    parent_id   TEXT,
                    title       TEXT NOT NULL,
                    body        TEXT,
                    status      TEXT NOT NULL DEFAULT 'todo',
                    repo_path   TEXT,
                    session_id  TEXT,
                    provider    TEXT,
                    external_id TEXT,
                    external_url TEXT,
                    sort_order  REAL NOT NULL DEFAULT 0,
                    pinned_pr_url TEXT,
                    cost        REAL,
                    created_at  INTEGER NOT NULL,
                    updated_at  INTEGER NOT NULL
                );
                CREATE TABLE work_item_runs (
                    id            TEXT PRIMARY KEY,
                    work_item_id  TEXT NOT NULL,
                    session_id    TEXT,
                    provider      TEXT,
                    profile_id    TEXT,
                    status        TEXT NOT NULL DEFAULT 'running',
                    worktree_path TEXT,
                    branch        TEXT,
                    cost          REAL,
                    created_at    INTEGER NOT NULL,
                    started_at    INTEGER,
                    ended_at      INTEGER,
                    updated_at    INTEGER NOT NULL,
                    FOREIGN KEY(work_item_id) REFERENCES work_items(id) ON DELETE CASCADE
                );
                CREATE TABLE work_item_run_events (
                    id         TEXT PRIMARY KEY,
                    run_id     TEXT NOT NULL,
                    kind       TEXT NOT NULL,
                    payload    TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY(run_id) REFERENCES work_item_runs(id) ON DELETE CASCADE
                );
                CREATE TABLE work_item_decisions (
                    id             TEXT PRIMARY KEY,
                    run_id         TEXT NOT NULL,
                    question       TEXT NOT NULL,
                    options        TEXT NOT NULL,
                    default_value  TEXT,
                    status         TEXT NOT NULL DEFAULT 'pending',
                    resolved_value TEXT,
                    resolved_by    TEXT,
                    created_at     INTEGER NOT NULL,
                    resolved_at    INTEGER,
                    updated_at     INTEGER NOT NULL,
                    timeout_at     INTEGER,
                    FOREIGN KEY(run_id) REFERENCES work_item_runs(id) ON DELETE CASCADE
                );
                PRAGMA user_version = 3;",
            )
            .unwrap();
        }

        let store = WorkItemStore::open(&path).unwrap();
        let version: i64 =
            store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 7);
        for column in [
            "repo_path",
            "agent_profile",
            "base_branch",
            "worktree_path",
            "branch",
            "fetch_first",
            "start_error",
        ] {
            assert!(table_has_column(&store.conn, "work_items", column).unwrap());
        }
        assert!(table_has_column(&store.conn, "work_item_runs", "kind").unwrap());
        assert!(table_has_column(&store.conn, "work_item_runs", "pty_id").unwrap());
        let item = store.get("missing").unwrap();
        assert!(item.is_none());
    }

    #[test]
    fn create_and_get_round_trip() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        let mut input = input("Fix bug");
        input.repo_path = Some("/repo".into());
        input.agent_profile = Some("claude".into());
        input.base_branch = Some("origin/main".into());
        input.branch = Some("feat/fix-bug".into());
        input.fetch_first = Some(true);
        let item = store.create("i-1".into(), input, 1000).unwrap();
        assert_eq!(item.id, "i-1");
        assert_eq!(item.title, "Fix bug");
        assert_eq!(item.status, WorkItemStatus::Todo);
        assert_eq!(item.repo_path.as_deref(), Some("/repo"));
        assert_eq!(item.agent_profile.as_deref(), Some("claude"));
        assert_eq!(item.base_branch.as_deref(), Some("origin/main"));
        assert_eq!(item.branch.as_deref(), Some("feat/fix-bug"));
        assert_eq!(item.fetch_first, Some(true));
        assert_eq!(item.created_at, 1000);

        let fetched = store.get("i-1").unwrap().unwrap();
        assert_eq!(fetched.title, "Fix bug");
        assert_eq!(fetched.branch.as_deref(), Some("feat/fix-bug"));
    }

    #[test]
    fn list_returns_all_items() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("A"), 1000).unwrap();
        store.create("i-2".into(), input("B"), 1001).unwrap();
        let items = store.list(None).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn list_filters_by_project_id() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        let mut a = input("A");
        a.project_id = Some("p-1".into());
        let mut b = input("B");
        b.project_id = Some("p-2".into());
        store.create("i-1".into(), a, 1000).unwrap();
        store.create("i-2".into(), b, 1001).unwrap();

        let items = store.list(Some("p-1")).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "i-1");
    }

    #[test]
    fn update_changes_fields() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Old"), 1000).unwrap();

        let mut upd = input("New");
        upd.body = Some("body text".into());
        upd.agent_profile = Some("codex".into());
        upd.base_branch = Some("main".into());
        upd.branch = Some("feat/new".into());
        upd.fetch_first = Some(false);
        let updated = store.update("i-1", upd, 2000).unwrap().unwrap();
        assert_eq!(updated.title, "New");
        assert_eq!(updated.body.as_deref(), Some("body text"));
        assert_eq!(updated.agent_profile.as_deref(), Some("codex"));
        assert_eq!(updated.base_branch.as_deref(), Some("main"));
        assert_eq!(updated.branch.as_deref(), Some("feat/new"));
        assert_eq!(updated.fetch_first, Some(false));
        assert_eq!(updated.updated_at, 2000);
    }

    #[test]
    fn update_clears_nullable_fields_when_json_null_is_present() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        let mut item = input("Old");
        item.project_id = Some("p-1".into());
        item.repo_path = Some("/repo".into());
        item.agent_profile = Some("claude".into());
        item.base_branch = Some("origin/main".into());
        item.worktree_path = Some("/repo/.worktrees/old".into());
        item.branch = Some("feat/old".into());
        item.fetch_first = Some(true);
        item.start_error = Some("missing repo".into());
        store.create("i-1".into(), item, 1000).unwrap();

        let upd: WorkItemInput = serde_json::from_value(serde_json::json!({
            "title": "New",
            "projectId": null,
            "repoPath": null,
            "baseBranch": null,
            "worktreePath": null,
            "branch": null,
            "fetchFirst": null
        }))
        .unwrap();
        let updated = store.update("i-1", upd, 2000).unwrap().unwrap();

        assert_eq!(updated.title, "New");
        assert_eq!(updated.project_id, None);
        assert_eq!(updated.repo_path, None);
        assert_eq!(updated.base_branch, None);
        assert_eq!(updated.worktree_path, None);
        assert_eq!(updated.branch, None);
        assert_eq!(updated.fetch_first, None);
        assert_eq!(updated.start_error, None);
    }

    #[test]
    fn update_preserves_start_error_unless_start_config_changes() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        let mut item = input("Old");
        item.start_error = Some("missing repo".into());
        item.agent_profile = Some("claude".into());
        store.create("i-1".into(), item, 1000).unwrap();

        let renamed = store.update("i-1", input("New"), 2000).unwrap().unwrap();
        assert_eq!(renamed.title, "New");
        assert_eq!(renamed.start_error.as_deref(), Some("missing repo"));

        let mut same_config = input("Newer");
        same_config.agent_profile = Some("claude".into());
        let same = store.update("i-1", same_config, 2500).unwrap().unwrap();
        assert_eq!(same.start_error.as_deref(), Some("missing repo"));

        let mut repo_update = input("New");
        repo_update.branch = Some("feat/new".into());
        let updated = store.update("i-1", repo_update, 3000).unwrap().unwrap();
        assert_eq!(updated.branch.as_deref(), Some("feat/new"));
        assert_eq!(updated.start_error, None);
    }

    #[test]
    fn move_item_updates_status_and_sort_order() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();

        let moved = store.move_item("i-1", WorkItemStatus::Doing, 1.5, 2000).unwrap().unwrap();
        assert_eq!(moved.status, WorkItemStatus::Doing);
        assert!((moved.sort_order - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn delete_removes_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();

        let deleted = store.delete("i-1").unwrap();
        assert!(deleted);
        assert!(store.get("i-1").unwrap().is_none());

        let not_deleted = store.delete("i-1").unwrap();
        assert!(!not_deleted, "second delete is a no-op");
    }

    #[test]
    fn set_session_binds_session_id() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();

        let bound = store.set_session("i-1", "sess-1", 2000).unwrap().unwrap();
        assert_eq!(bound.session_id.as_deref(), Some("sess-1"));
    }

    #[test]
    fn set_session_rejects_session_bound_to_another_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task one"), 1000).unwrap();
        store.create("i-2".into(), input("Task two"), 1001).unwrap();
        store.set_session("i-1", "sess-1", 2000).unwrap().unwrap();

        let err = store.set_session("i-2", "sess-1", 3000).unwrap_err();

        assert!(err.to_string().contains("session already bound"), "unexpected error: {err}");
        let first = store.get("i-1").unwrap().unwrap();
        let second = store.get("i-2").unwrap().unwrap();
        assert_eq!(first.session_id.as_deref(), Some("sess-1"));
        assert!(second.session_id.is_none());
    }

    #[test]
    fn set_session_if_unbound_only_binds_once() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();

        // First dispatch wins.
        assert!(store.set_session_if_unbound("i-1", "sess-1", 2000).unwrap());
        // A racing second dispatch loses and must not clobber the binding.
        assert!(!store.set_session_if_unbound("i-1", "sess-2", 3000).unwrap());

        let item = store.get("i-1").unwrap().unwrap();
        assert_eq!(item.session_id.as_deref(), Some("sess-1"));
    }

    #[test]
    fn set_session_if_unbound_is_false_for_missing_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        assert!(!store.set_session_if_unbound("nope", "sess-1", 1000).unwrap());
    }

    #[test]
    fn set_session_if_unbound_rejects_session_bound_to_another_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task one"), 1000).unwrap();
        store.create("i-2".into(), input("Task two"), 1001).unwrap();
        store.set_session("i-1", "sess-1", 2000).unwrap().unwrap();

        let err = store.set_session_if_unbound("i-2", "sess-1", 3000).unwrap_err();

        assert!(err.to_string().contains("session already bound"), "unexpected error: {err}");
        let first = store.get("i-1").unwrap().unwrap();
        let second = store.get("i-2").unwrap().unwrap();
        assert_eq!(first.session_id.as_deref(), Some("sess-1"));
        assert!(second.session_id.is_none());
    }

    #[test]
    fn detach_session_clears_only_matching_item_binding() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task one"), 1000).unwrap();
        store.create("i-2".into(), input("Task two"), 1001).unwrap();
        store.set_session("i-1", "sess-1", 2000).unwrap().unwrap();
        store.set_session("i-2", "sess-2", 2001).unwrap().unwrap();

        let detached = store.detach_session("i-1", 3000).unwrap().unwrap();

        assert!(detached.session_id.is_none());
        let other = store.get("i-2").unwrap().unwrap();
        assert_eq!(other.session_id.as_deref(), Some("sess-2"));
    }

    #[test]
    fn complete_start_rejects_session_bound_to_another_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task one"), 1000).unwrap();
        store.create("i-2".into(), input("Task two"), 1001).unwrap();
        store.set_session("i-1", "sess-1", 2000).unwrap().unwrap();

        let err =
            store.complete_start("i-2", "sess-1", None, None, None, None, 0.0, 3000).unwrap_err();

        assert!(err.to_string().contains("session already bound"), "unexpected error: {err}");
        let second = store.get("i-2").unwrap().unwrap();
        assert!(second.session_id.is_none());
    }

    #[test]
    fn dispatch_run_rejects_session_bound_to_another_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task one"), 1000).unwrap();
        store.create("i-2".into(), input("Task two"), 1001).unwrap();
        store.set_session("i-1", "sess-1", 2000).unwrap().unwrap();

        let err = store
            .dispatch_run("run-1".into(), "i-2", "sess-1", None, None, None, None, 0.0, 3000)
            .unwrap_err();

        assert!(err.to_string().contains("session already bound"), "unexpected error: {err}");
        let second = store.get("i-2").unwrap().unwrap();
        assert!(second.session_id.is_none());
        assert!(store.list_runs(Some("i-2")).unwrap().is_empty());
    }

    #[test]
    fn dispatch_run_rolls_back_item_update_when_run_insert_fails() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();

        let err = store
            .dispatch_run("run-1".into(), "i-1", "sess-2", None, None, None, None, 5.0, 1200)
            .unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));

        let item = store.get("i-1").unwrap().unwrap();
        assert_eq!(item.session_id, None);
        assert_eq!(item.status, WorkItemStatus::Todo);
        assert_eq!(item.sort_order, 0.0);
    }

    #[test]
    fn dispatch_run_updates_item_and_creates_run_atomically() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();

        let (item, run) = store
            .dispatch_run(
                "run-1".into(),
                "i-1",
                "sess-1",
                Some("claude"),
                Some("claude"),
                Some("/repo"),
                Some("branch"),
                7.0,
                1200,
            )
            .unwrap()
            .expect("item should exist");

        assert_eq!(item.session_id.as_deref(), Some("sess-1"));
        assert_eq!(item.status, WorkItemStatus::Doing);
        assert_eq!(item.sort_order, 7.0);
        assert_eq!(run.session_id.as_deref(), Some("sess-1"));
        assert_eq!(run.work_item_id, "i-1");
    }

    #[test]
    fn dispatch_run_rejects_already_bound_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .dispatch_run("run-1".into(), "i-1", "sess-1", None, None, None, None, 0.0, 1100)
            .unwrap()
            .expect("first dispatch should bind");

        let result = store
            .dispatch_run("run-2".into(), "i-1", "sess-2", None, None, None, None, 0.0, 1200)
            .unwrap();

        assert!(result.is_none());
        let item = store.get("i-1").unwrap().unwrap();
        assert_eq!(item.session_id.as_deref(), Some("sess-1"));
        assert!(store.get_run("run-2").unwrap().is_none());
    }

    #[test]
    fn runs_are_persisted_and_listed_by_work_item() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();

        let run = store
            .create_run(
                "run-1".into(),
                "i-1",
                Some("sess-1"),
                Some("claude"),
                Some("claude"),
                Some("/repo/.roux/worktrees/run-1"),
                Some("roux/run-1"),
                1100,
            )
            .unwrap();

        assert_eq!(run.work_item_id, "i-1");
        assert_eq!(run.kind, roux_core::WorkItemRunKind::Implementation);
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Running);
        assert_eq!(run.session_id.as_deref(), Some("sess-1"));

        let runs = store.list_runs(Some("i-1")).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run-1");
    }

    #[test]
    fn run_pty_id_persists_and_lists() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        let run = store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();
        assert!(run.pty_id.is_none());

        let run = store.set_run_pty_id("run-1", Some("sess-1-run-1"), 1200).unwrap().unwrap();
        assert_eq!(run.pty_id.as_deref(), Some("sess-1-run-1"));

        let runs = store.list_runs(Some("i-1")).unwrap();
        assert_eq!(runs[0].pty_id.as_deref(), Some("sess-1-run-1"));
    }

    #[test]
    fn runs_are_listed_in_insert_order_even_with_same_timestamp() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run_with_status(
                "run-2".into(),
                "i-1",
                Some("sess-2"),
                None,
                None,
                None,
                None,
                WorkItemRunStatus::Done,
                1100,
            )
            .unwrap();
        store
            .create_run_with_status(
                "run-1".into(),
                "i-1",
                Some("sess-1"),
                None,
                None,
                None,
                None,
                WorkItemRunStatus::Failed,
                1100,
            )
            .unwrap();

        let runs = store.list_runs(Some("i-1")).unwrap();
        assert_eq!(runs.iter().map(|run| run.id.as_str()).collect::<Vec<_>>(), ["run-2", "run-1"]);
    }

    #[test]
    fn run_events_are_append_only_and_ordered() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();

        store
            .append_run_event(
                "event-1".into(),
                "run-1",
                roux_core::WorkItemRunEventKind::Text,
                serde_json::json!({ "text": "hello" }),
                1200,
            )
            .unwrap();
        store
            .append_run_event(
                "event-2".into(),
                "run-1",
                roux_core::WorkItemRunEventKind::Result,
                serde_json::json!({ "ok": true }),
                1201,
            )
            .unwrap();

        let events = store.list_run_events("run-1").unwrap();
        assert_eq!(
            events.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["event-1", "event-2"]
        );
        assert_eq!(events[0].kind, roux_core::WorkItemRunEventKind::Text);
        assert_eq!(events[0].payload["text"], "hello");
    }

    #[test]
    fn run_status_update_persists_terminal_status_and_event() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();

        let (run, event) = store
            .update_run_status_with_event(
                "run-1",
                roux_core::WorkItemRunStatus::Stopped,
                "event-1".into(),
                serde_json::json!({ "status": "stopped", "reason": "user" }),
                1200,
            )
            .unwrap()
            .expect("run should exist");

        assert_eq!(run.status, roux_core::WorkItemRunStatus::Stopped);
        assert_eq!(run.ended_at, Some(1200));
        assert_eq!(event.kind, roux_core::WorkItemRunEventKind::StatusChanged);
        assert_eq!(event.payload["reason"], "user");
    }

    #[test]
    fn run_status_update_does_not_overwrite_terminal_status() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();
        store
            .update_run_status_with_event(
                "run-1",
                roux_core::WorkItemRunStatus::Done,
                "event-1".into(),
                serde_json::json!({ "status": "done" }),
                1200,
            )
            .unwrap()
            .expect("first update should win");

        let result = store
            .update_run_status_with_event(
                "run-1",
                roux_core::WorkItemRunStatus::Stopped,
                "event-2".into(),
                serde_json::json!({ "status": "stopped", "reason": "user" }),
                1300,
            )
            .unwrap();

        assert!(result.is_none());
        let run = store.get_run("run-1").unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Done);
        let events = store.list_run_events("run-1").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "event-1");
    }

    #[test]
    fn decisions_can_be_created_and_resolved_with_audit_events() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();

        let decision = store
            .create_decision(
                "dec-1".into(),
                "run-1",
                "Choose path?",
                vec![
                    roux_core::WorkItemDecisionOption {
                        value: "existing".into(),
                        label: "Use existing file".into(),
                    },
                    roux_core::WorkItemDecisionOption {
                        value: "new".into(),
                        label: "Create new file".into(),
                    },
                ],
                Some("existing"),
                None,
                1200,
            )
            .unwrap();

        assert_eq!(decision.status, roux_core::WorkItemDecisionStatus::Pending);
        assert_eq!(store.list_pending_decisions(Some("i-1")).unwrap().len(), 1);

        let resolved = store
            .resolve_decision("dec-1", "new", Some("user"), 1300)
            .unwrap()
            .expect("decision should resolve");

        assert_eq!(resolved.status, roux_core::WorkItemDecisionStatus::Resolved);
        assert_eq!(resolved.resolved_value.as_deref(), Some("new"));
        assert_eq!(store.list_pending_decisions(Some("i-1")).unwrap().len(), 0);
    }

    #[test]
    fn due_decisions_timeout_to_default_and_unblock_run() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();

        let decision = store
            .create_decision(
                "dec-1".into(),
                "run-1",
                "Choose path?",
                vec![roux_core::WorkItemDecisionOption {
                    value: "existing".into(),
                    label: "Use existing file".into(),
                }],
                Some("existing"),
                Some(1250),
                1200,
            )
            .unwrap();

        assert_eq!(decision.timeout_at, Some(1250));
        assert_eq!(store.timeout_due_decisions(1249).unwrap().len(), 0);

        let timed_out = store.timeout_due_decisions(1250).unwrap();
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0].status, roux_core::WorkItemDecisionStatus::TimedOut);
        assert_eq!(timed_out[0].resolved_value.as_deref(), Some("existing"));
        assert_eq!(timed_out[0].resolved_by.as_deref(), Some("timeout"));
        assert_eq!(store.list_pending_decisions(Some("i-1")).unwrap().len(), 0);

        let run = store.get_run("run-1").unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Running);
    }

    #[test]
    fn resolve_decision_ignores_non_pending_decisions() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();
        store
            .create_decision(
                "dec-1".into(),
                "run-1",
                "Choose path?",
                vec![roux_core::WorkItemDecisionOption {
                    value: "existing".into(),
                    label: "Use existing file".into(),
                }],
                Some("existing"),
                Some(1250),
                1200,
            )
            .unwrap();
        store.timeout_decision_to_default("dec-1", 1250).unwrap().unwrap();

        let result = store.resolve_decision("dec-1", "manual", Some("user"), 1300).unwrap();

        assert!(result.is_none());
        let decision = store.get_decision("dec-1").unwrap().unwrap();
        assert_eq!(decision.status, roux_core::WorkItemDecisionStatus::TimedOut);
        assert_eq!(decision.resolved_value.as_deref(), Some("existing"));
        assert_eq!(decision.resolved_by.as_deref(), Some("timeout"));
    }

    #[test]
    fn create_decision_does_not_revive_terminal_run() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();
        store
            .update_run_status_with_event(
                "run-1",
                roux_core::WorkItemRunStatus::Done,
                "event-1".into(),
                serde_json::json!({ "status": "done" }),
                1200,
            )
            .unwrap()
            .expect("run should update");

        let err = store
            .create_decision(
                "dec-1".into(),
                "run-1",
                "Choose path?",
                vec![WorkItemDecisionOption { value: "a".into(), label: "A".into() }],
                None,
                None,
                1300,
            )
            .unwrap_err();

        assert!(matches!(err, rusqlite::Error::QueryReturnedNoRows));
        let run = store.get_run("run-1").unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Done);
        assert!(store.list_pending_decisions(None).unwrap().is_empty());
    }

    #[test]
    fn runs_events_and_pending_decisions_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.db");
        {
            let mut store = WorkItemStore::open(&path).unwrap();
            store.create("i-1".into(), input("Task"), 1000).unwrap();
            store
                .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
                .unwrap();
            store
                .append_run_event(
                    "event-1".into(),
                    "run-1",
                    roux_core::WorkItemRunEventKind::Text,
                    serde_json::json!({ "text": "hello" }),
                    1200,
                )
                .unwrap();
            store
                .create_decision(
                    "dec-1".into(),
                    "run-1",
                    "Choose path?",
                    vec![roux_core::WorkItemDecisionOption {
                        value: "go".into(),
                        label: "Go".into(),
                    }],
                    Some("go"),
                    None,
                    1300,
                )
                .unwrap();
        }

        let store = WorkItemStore::open(&path).unwrap();
        let runs = store.list_runs(Some("i-1")).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run-1");
        let events = store.list_run_events("run-1").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["text"], "hello");
        let decisions = store.list_pending_decisions(Some("i-1")).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].id, "dec-1");
    }

    #[test]
    fn resolving_one_of_multiple_pending_decisions_keeps_run_blocked() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();
        let options =
            vec![roux_core::WorkItemDecisionOption { value: "go".into(), label: "Go".into() }];
        store
            .create_decision("dec-1".into(), "run-1", "First?", options.clone(), None, None, 1200)
            .unwrap();
        store
            .create_decision("dec-2".into(), "run-1", "Second?", options, None, None, 1201)
            .unwrap();

        store.resolve_decision("dec-1", "go", Some("user"), 1300).unwrap();
        let run = store.get_run("run-1").unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Blocked);

        store.resolve_decision("dec-2", "go", Some("user"), 1400).unwrap();
        let run = store.get_run("run-1").unwrap().unwrap();
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Running);
    }

    #[test]
    fn deleting_work_item_cascades_run_history_and_decisions() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
            .unwrap();
        store
            .append_run_event(
                "event-1".into(),
                "run-1",
                roux_core::WorkItemRunEventKind::Decision,
                serde_json::json!({ "question": "Choose?" }),
                1200,
            )
            .unwrap();
        store
            .create_decision(
                "dec-1".into(),
                "run-1",
                "Choose path?",
                vec![roux_core::WorkItemDecisionOption { value: "go".into(), label: "Go".into() }],
                Some("go"),
                None,
                1300,
            )
            .unwrap();

        assert!(store.delete("i-1").unwrap());
        assert!(store.list_runs(None).unwrap().is_empty());
        assert!(store.list_run_events("run-1").unwrap().is_empty());
        assert!(store.list_pending_decisions(None).unwrap().is_empty());
    }

    #[test]
    fn upsert_by_external_inserts_when_no_match() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        let item = store
            .upsert_by_external("i-1".into(), input_with_ref("New", "gh", "123"), 1000)
            .unwrap();
        assert_eq!(item.id, "i-1");
        assert_eq!(item.provider.as_deref(), Some("gh"));
    }

    #[test]
    fn upsert_by_external_deduplicates_on_provider_and_external_id() {
        let mut store = WorkItemStore::open_in_memory().unwrap();

        // First import
        store
            .upsert_by_external("i-1".into(), input_with_ref("Old title", "gh", "123"), 1000)
            .unwrap();

        // Re-import with updated title
        let updated = store
            .upsert_by_external("i-NEW".into(), input_with_ref("New title", "gh", "123"), 2000)
            .unwrap();

        // Should have updated the ORIGINAL item, not created a duplicate
        assert_eq!(updated.id, "i-1", "must update original item, not insert new one");
        assert_eq!(updated.title, "New title");
        assert_eq!(updated.updated_at, 2000);

        let items = store.list(None).unwrap();
        assert_eq!(items.len(), 1, "no duplicates");
    }

    #[test]
    fn upsert_without_external_ref_always_inserts() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.upsert_by_external("i-1".into(), input("A"), 1000).unwrap();
        store.upsert_by_external("i-2".into(), input("B"), 1001).unwrap();
        let items = store.list(None).unwrap();
        assert_eq!(items.len(), 2);
    }
}
