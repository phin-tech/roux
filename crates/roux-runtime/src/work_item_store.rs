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

use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use serde_json::Value;

use roux_core::{
    ExternalRef, WorkItem, WorkItemDecision, WorkItemDecisionOption, WorkItemDecisionStatus,
    WorkItemInput, WorkItemRun, WorkItemRunEvent, WorkItemRunEventKind, WorkItemRunStatus,
    WorkItemStatus,
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
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS work_items (
                    id          TEXT PRIMARY KEY,
                    project_id  TEXT,
                    parent_id   TEXT,
                    title       TEXT NOT NULL,
                    body        TEXT,
                    status      TEXT NOT NULL DEFAULT 'todo',
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
        }
        if version < 2 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS work_item_runs (
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
        }
        if version < 3 {
            conn.execute_batch(
                "ALTER TABLE work_item_decisions ADD COLUMN timeout_at INTEGER;
                CREATE INDEX IF NOT EXISTS work_item_decisions_timeout
                    ON work_item_decisions(timeout_at)
                    WHERE status = 'pending' AND timeout_at IS NOT NULL;
                PRAGMA user_version = 3;",
            )?;
        }
        Ok(WorkItemStore { conn })
    }

    pub fn list(&self, project_id: Option<&str>) -> SqlResult<Vec<WorkItem>> {
        let sql = if project_id.is_some() {
            "SELECT id, project_id, parent_id, title, body, status, session_id,
                    provider, external_id, external_url, sort_order, pinned_pr_url,
                    cost, created_at, updated_at
             FROM work_items
             WHERE project_id = ?1
             ORDER BY sort_order, created_at"
        } else {
            "SELECT id, project_id, parent_id, title, body, status, session_id,
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
                "SELECT id, project_id, parent_id, title, body, status, session_id,
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
             (id, project_id, parent_id, title, body, status, provider,
              external_id, external_url, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                input.project_id,
                input.parent_id,
                input.title,
                input.body,
                status,
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
        let status = input.status.as_ref().map(|s| s.as_str().to_string());
        let (provider, external_id, external_url) = split_external_ref(input.external_ref.as_ref());
        self.conn.execute(
            "UPDATE work_items SET
                title       = ?2,
                body        = COALESCE(?3, body),
                status      = COALESCE(?4, status),
                project_id  = COALESCE(?5, project_id),
                parent_id   = COALESCE(?6, parent_id),
                provider    = COALESCE(?7, provider),
                external_id = COALESCE(?8, external_id),
                external_url = COALESCE(?9, external_url),
                sort_order  = COALESCE(?10, sort_order),
                updated_at  = ?11
             WHERE id = ?1",
            params![
                id,
                input.title,
                input.body,
                status,
                input.project_id,
                input.parent_id,
                provider,
                external_id,
                external_url,
                input.sort_order,
                now as i64,
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

    pub fn set_session(
        &mut self,
        id: &str,
        session_id: &str,
        now: u64,
    ) -> SqlResult<Option<WorkItem>> {
        self.conn.execute(
            "UPDATE work_items SET session_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, session_id, now as i64],
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
        let changed = self.conn.execute(
            "UPDATE work_items SET session_id = ?2, updated_at = ?3
             WHERE id = ?1 AND session_id IS NULL",
            params![id, session_id, now as i64],
        )?;
        Ok(changed > 0)
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
        self.conn.execute(
            "INSERT INTO work_item_runs
             (id, work_item_id, session_id, provider, profile_id, status,
              worktree_path, branch, created_at, started_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                work_item_id,
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
        self.get_run(&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn get_run(&self, id: &str) -> SqlResult<Option<WorkItemRun>> {
        self.conn
            .query_row(
                "SELECT id, work_item_id, session_id, provider, profile_id, status,
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
            "SELECT id, work_item_id, session_id, provider, profile_id, status,
                    worktree_path, branch, cost, created_at, started_at, ended_at, updated_at
             FROM work_item_runs
             WHERE work_item_id = ?1
             ORDER BY rowid"
        } else {
            "SELECT id, work_item_id, session_id, provider, profile_id, status,
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
            WorkItemRunStatus::Failed | WorkItemRunStatus::Stopped | WorkItemRunStatus::Done
        );
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE work_item_runs
             SET status = ?2,
                 updated_at = ?3,
                 ended_at = CASE WHEN ?4 THEN COALESCE(ended_at, ?3) ELSE ended_at END
             WHERE id = ?1",
            params![id, status.as_str(), now as i64, is_terminal],
        )?;
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
        tx.execute(
            "UPDATE work_item_runs SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![run_id, WorkItemRunStatus::Blocked.as_str(), now as i64],
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
        session_id: row.get(6)?,
        provider: row.get(7)?,
        external_id: row.get(8)?,
        external_url: row.get(9)?,
        sort_order: row.get(10)?,
        pinned_pr_url: row.get(11)?,
        cost: row.get(12)?,
        created_at: row.get::<_, i64>(13)? as u64,
        updated_at: row.get::<_, i64>(14)? as u64,
    })
}

fn row_to_work_item_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItemRun> {
    let status_str: String = row.get(5)?;
    let status = WorkItemRunStatus::from_str_opt(&status_str).unwrap_or_default();
    Ok(WorkItemRun {
        id: row.get(0)?,
        work_item_id: row.get(1)?,
        session_id: row.get(2)?,
        provider: row.get(3)?,
        profile_id: row.get(4)?,
        status,
        worktree_path: row.get(6)?,
        branch: row.get(7)?,
        cost: row.get(8)?,
        created_at: row.get::<_, i64>(9)? as u64,
        started_at: row.get::<_, Option<i64>>(10)?.map(|value| value as u64),
        ended_at: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
        updated_at: row.get::<_, i64>(12)? as u64,
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
        assert_eq!(version, 3);
    }

    #[test]
    fn create_and_get_round_trip() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        let item = store.create("i-1".into(), input("Fix bug"), 1000).unwrap();
        assert_eq!(item.id, "i-1");
        assert_eq!(item.title, "Fix bug");
        assert_eq!(item.status, WorkItemStatus::Todo);
        assert_eq!(item.created_at, 1000);

        let fetched = store.get("i-1").unwrap().unwrap();
        assert_eq!(fetched.title, "Fix bug");
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
        let updated = store.update("i-1", upd, 2000).unwrap().unwrap();
        assert_eq!(updated.title, "New");
        assert_eq!(updated.body.as_deref(), Some("body text"));
        assert_eq!(updated.updated_at, 2000);
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
        assert_eq!(run.status, roux_core::WorkItemRunStatus::Running);
        assert_eq!(run.session_id.as_deref(), Some("sess-1"));

        let runs = store.list_runs(Some("i-1")).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run-1");
    }

    #[test]
    fn runs_are_listed_in_insert_order_even_with_same_timestamp() {
        let mut store = WorkItemStore::open_in_memory().unwrap();
        store.create("i-1".into(), input("Task"), 1000).unwrap();
        store
            .create_run("run-2".into(), "i-1", Some("sess-2"), None, None, None, None, 1100)
            .unwrap();
        store
            .create_run("run-1".into(), "i-1", Some("sess-1"), None, None, None, None, 1100)
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
