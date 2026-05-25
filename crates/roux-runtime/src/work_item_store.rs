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

use roux_core::{ExternalRef, WorkItem, WorkItemInput, WorkItemStatus};

pub struct WorkItemStore {
    conn: Connection,
}

impl WorkItemStore {
    pub fn open(path: &Path) -> SqlResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(e.to_string()),
                ))?;
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
             PRAGMA busy_timeout=5000;",
        )?;
        let version: i64 =
            conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
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
        self.get(&id)?.ok_or_else(|| {
            rusqlite::Error::QueryReturnedNoRows
        })
    }

    pub fn update(&mut self, id: &str, input: WorkItemInput, now: u64) -> SqlResult<Option<WorkItem>> {
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

    pub fn set_session(&mut self, id: &str, session_id: &str, now: u64) -> SqlResult<Option<WorkItem>> {
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

}

fn split_external_ref(
    r: Option<&ExternalRef>,
) -> (Option<String>, Option<String>, Option<String>) {
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
    fn migration_sets_user_version_to_one() {
        let store = WorkItemStore::open_in_memory().unwrap();
        let version: i64 =
            store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 1);
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

        let moved = store
            .move_item("i-1", WorkItemStatus::Doing, 1.5, 2000)
            .unwrap()
            .unwrap();
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
