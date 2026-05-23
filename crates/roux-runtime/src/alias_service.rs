use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use roux_core::{AgentAlias, ConsumptionMode};

use crate::alias_persistence::{self, load_from_path, save_to_path};
use crate::alias_store::{
    AliasStore, BindError, BindRequest, GroupError, PaneUnbindResult, ProjectFilter,
};

#[derive(Clone)]
pub struct AliasManager {
    inner: Arc<Mutex<AliasStore>>,
    persistence_path: Option<Arc<PathBuf>>,
}

impl AliasManager {
    pub fn load() -> Self {
        Self::load_from(alias_persistence::persistence_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let entries = load_from_path(&path);
        let had_me = entries.iter().any(|alias| alias.alias == "me" && alias.project_id.is_none());
        let mut store = AliasStore::from_entries(entries);
        if !had_me {
            store.ensure("me", None);
        }
        let manager =
            Self { inner: Arc::new(Mutex::new(store)), persistence_path: Some(Arc::new(path)) };
        if !had_me {
            manager.persist();
        }
        manager
    }

    pub fn in_memory() -> Self {
        let mut store = AliasStore::new();
        store.ensure("me", None);
        Self { inner: Arc::new(Mutex::new(store)), persistence_path: None }
    }

    pub fn bind(&self, canonical: &str, req: BindRequest) -> Result<AgentAlias, BindError> {
        let alias = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.bind(canonical, req)?
        };
        self.persist();
        Ok(alias)
    }

    pub fn unbind(&self, canonical: &str, project_id: Option<&str>) -> bool {
        let changed = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.unbind(canonical, project_id)
        };
        if changed {
            self.persist();
        }
        changed
    }

    pub fn ensure(&self, canonical: &str, project_id: Option<String>) -> AgentAlias {
        let (alias, was_new) = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            let before_len = store.entries().len();
            let alias = store.ensure(canonical, project_id);
            let was_new = store.entries().len() > before_len;
            (alias, was_new)
        };
        if was_new {
            self.persist();
        }
        alias
    }

    pub fn get(&self, canonical: &str, project_id: Option<&str>) -> Option<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.get(canonical, project_id).cloned()
    }

    pub fn find_all_by_name(&self, canonical: &str) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.find_all_by_name(canonical).into_iter().cloned().collect()
    }

    pub fn list(&self, project_filter: ProjectFilter<'_>, only_unbound: bool) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.list(project_filter, only_unbound)
    }

    pub fn whoami(&self, session_id: &str) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.whoami(session_id)
    }

    pub fn find_for_pane(&self, pane_id: &str) -> Vec<AgentAlias> {
        let store = self.inner.lock().expect("alias store poisoned");
        store.find_for_pane(pane_id).into_iter().cloned().collect()
    }

    pub fn unbind_for_pane(&self, pane_id: &str, only_auto_claimed: bool) -> Vec<PaneUnbindResult> {
        let released = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.unbind_for_pane(pane_id, only_auto_claimed)
        };
        if !released.is_empty() {
            self.persist();
        }
        released
    }

    pub fn add_member(
        &self,
        canonical: &str,
        project_id: Option<&str>,
        pane_id: &str,
    ) -> Result<AgentAlias, GroupError> {
        let (alias, changed) = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            let was_new_row = store.get(canonical, project_id).is_none();
            if was_new_row {
                store.ensure(canonical, project_id.map(String::from));
            }
            let before =
                store.get(canonical, project_id).map(|alias| alias.members.len()).unwrap_or(0);
            let alias = store.add_member(canonical, project_id, pane_id)?;
            let added = alias.members.len() != before;
            (alias, was_new_row || added)
        };
        if changed {
            self.persist();
        }
        Ok(alias)
    }

    pub fn remove_member(
        &self,
        canonical: &str,
        project_id: Option<&str>,
        pane_id: &str,
    ) -> Result<bool, GroupError> {
        let changed = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            store.remove_member(canonical, project_id, pane_id)?
        };
        if changed {
            self.persist();
        }
        Ok(changed)
    }

    pub fn set_consumption_mode(
        &self,
        canonical: &str,
        project_id: Option<&str>,
        mode: ConsumptionMode,
    ) -> Result<AgentAlias, GroupError> {
        let (alias, changed) = {
            let mut store = self.inner.lock().expect("alias store poisoned");
            let before = store.get(canonical, project_id).map(|alias| alias.consumption_mode);
            let alias = store.set_consumption_mode(canonical, project_id, mode)?;
            let changed = before != Some(alias.consumption_mode);
            (alias, changed)
        };
        if changed {
            self.persist();
        }
        Ok(alias)
    }

    fn persist(&self) {
        let Some(path) = self.persistence_path.as_ref() else {
            return;
        };
        let store = self.inner.lock().expect("alias store poisoned");
        if let Err(err) = save_to_path(store.entries(), path.as_path()) {
            eprintln!("[roux] alias persistence failed at {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_seeds_me_alias_when_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let manager = AliasManager::load_from(path.clone());
        assert!(manager.get("me", None).is_some(), "me alias must be seeded");
        assert!(path.exists(), "load_from must write the seeded state");
    }

    #[test]
    fn bind_persists_and_reload_preserves_alias() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("aliases.json");
        let manager = AliasManager::load_from(path.clone());
        manager
            .bind(
                "reviewer",
                BindRequest { session_id: Some("session-1".into()), ..Default::default() },
            )
            .unwrap();

        let reloaded = AliasManager::load_from(path);
        assert_eq!(
            reloaded.get("reviewer", None).unwrap().session_id.as_deref(),
            Some("session-1")
        );
    }
}
