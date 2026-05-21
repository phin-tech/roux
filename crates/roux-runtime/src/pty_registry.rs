use std::collections::HashMap;

use roux_core::PtyInfo;

use crate::pty_lifecycle::{
    apply_metadata_command, PtyMetadataCommand, PtyMetadataCommandResult,
};
use crate::pty_session::PtySessionMetadata;

pub trait PtySessionRegistryEntry {
    fn metadata(&self) -> &PtySessionMetadata;
    fn metadata_mut(&mut self) -> &mut PtySessionMetadata;
    fn generation(&self) -> u64;

    fn touch_last_activity(&mut self) {}
}

pub struct PtySessionRegistry<Entry> {
    sessions: HashMap<String, Entry>,
}

impl<Entry> PtySessionRegistry<Entry> {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }

    pub fn insert(&mut self, pty_id: String, entry: Entry) -> Option<Entry> {
        self.sessions.insert(pty_id, entry)
    }

    pub fn remove(&mut self, pty_id: &str) -> Option<Entry> {
        self.sessions.remove(pty_id)
    }

    pub fn get(&self, pty_id: &str) -> Option<&Entry> {
        self.sessions.get(pty_id)
    }

    pub fn get_mut(&mut self, pty_id: &str) -> Option<&mut Entry> {
        self.sessions.get_mut(pty_id)
    }

    pub fn contains_key(&self, pty_id: &str) -> bool {
        self.sessions.contains_key(pty_id)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.sessions.keys()
    }
}

impl<Entry> Default for PtySessionRegistry<Entry> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Entry: PtySessionRegistryEntry> PtySessionRegistry<Entry> {
    pub fn apply_metadata_command(
        &mut self,
        command: &PtyMetadataCommand,
    ) -> PtyMetadataCommandResult {
        let Some(entry) = self.sessions.get_mut(command.pty_id()) else {
            return PtyMetadataCommandResult::Missing;
        };

        let generation = entry.generation();
        let result = apply_metadata_command(entry.metadata_mut(), generation, command);
        if matches!(result, PtyMetadataCommandResult::Applied)
            && matches!(command, PtyMetadataCommand::AttachToPane { .. })
        {
            entry.touch_last_activity();
        }
        result
    }

    pub fn generation(&self, pty_id: &str) -> Option<u64> {
        self.sessions.get(pty_id).map(PtySessionRegistryEntry::generation)
    }

    pub fn get_info(&self, pty_id: &str) -> Option<PtyInfo> {
        self.sessions.get(pty_id).map(|entry| entry.metadata().to_info(pty_id))
    }

    pub fn ids_for_session(&self, session_id: &str) -> Vec<String> {
        self.sessions
            .iter()
            .filter(|(_, entry)| entry.metadata().belongs_to_session(session_id))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn list_for_session(&self, session_id: &str) -> Vec<PtyInfo> {
        self.sessions
            .iter()
            .filter(|(_, entry)| entry.metadata().belongs_to_session(session_id))
            .map(|(id, entry)| entry.metadata().to_info(id))
            .collect()
    }

    pub fn list_all(&self) -> Vec<PtyInfo> {
        self.sessions
            .iter()
            .map(|(id, entry)| entry.metadata().to_info(id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{PtyRole, PtyStatus};

    #[derive(Debug)]
    struct TestEntry {
        metadata: PtySessionMetadata,
        generation: u64,
        touch_count: usize,
    }

    impl TestEntry {
        fn new(session_id: Option<&str>, generation: u64) -> Self {
            Self {
                metadata: PtySessionMetadata::new(crate::pty_session::PtySessionMetadataInputs {
                    role: PtyRole::Secondary,
                    pane_id: None,
                    detached_since_ms: 1,
                    session_id,
                    working_dir: Some("/repo"),
                    profile: Some("plain-shell"),
                    last_size: (80, 24),
                }),
                generation,
                touch_count: 0,
            }
        }
    }

    impl PtySessionRegistryEntry for TestEntry {
        fn metadata(&self) -> &PtySessionMetadata {
            &self.metadata
        }

        fn metadata_mut(&mut self) -> &mut PtySessionMetadata {
            &mut self.metadata
        }

        fn generation(&self) -> u64 {
            self.generation
        }

        fn touch_last_activity(&mut self) {
            self.touch_count += 1;
        }
    }

    #[test]
    fn registry_lists_and_filters_by_session() {
        let mut registry = PtySessionRegistry::new();
        registry.insert("pty-a".to_string(), TestEntry::new(Some("session-a"), 1));
        registry.insert("pty-b".to_string(), TestEntry::new(Some("session-b"), 1));
        registry.insert("detached".to_string(), TestEntry::new(None, 1));

        let infos = registry.list_for_session("session-a");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "pty-a");
        assert_eq!(registry.ids_for_session("session-b"), vec!["pty-b".to_string()]);
        assert_eq!(registry.list_all().len(), 3);
    }

    #[test]
    fn registry_applies_metadata_commands_and_touches_on_attach() {
        let mut registry = PtySessionRegistry::new();
        registry.insert("pty-a".to_string(), TestEntry::new(Some("session-a"), 7));

        assert_eq!(
            registry.apply_metadata_command(&PtyMetadataCommand::SetUnreadOutput {
                pty_id: "pty-a".to_string(),
                value: true,
            }),
            PtyMetadataCommandResult::Applied
        );
        assert!(registry.get("pty-a").unwrap().metadata.unread_output);
        assert_eq!(registry.get("pty-a").unwrap().touch_count, 0);

        assert_eq!(
            registry.apply_metadata_command(&PtyMetadataCommand::AttachToPane {
                pty_id: "pty-a".to_string(),
                pane_id: "pane-a".to_string(),
            }),
            PtyMetadataCommandResult::Applied
        );
        let entry = registry.get("pty-a").unwrap();
        assert!(matches!(
            entry.metadata.status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-a"
        ));
        assert!(!entry.metadata.unread_output);
        assert_eq!(entry.touch_count, 1);
    }

    #[test]
    fn registry_rejects_stale_exit_generation() {
        let mut registry = PtySessionRegistry::new();
        registry.insert("pty-a".to_string(), TestEntry::new(Some("session-a"), 7));

        assert_eq!(
            registry.apply_metadata_command(
                &PtyMetadataCommand::MarkExitedIfGenerationMatches {
                    pty_id: "pty-a".to_string(),
                    generation: 6,
                    code: Some(1),
                    at_ms: 99,
                },
            ),
            PtyMetadataCommandResult::StaleGeneration
        );
        assert!(matches!(
            registry.get("pty-a").unwrap().metadata.status,
            PtyStatus::RunningDetached { .. }
        ));
    }

    #[test]
    fn registry_reports_missing_metadata_targets() {
        let mut registry: PtySessionRegistry<TestEntry> = PtySessionRegistry::new();

        assert_eq!(
            registry.apply_metadata_command(&PtyMetadataCommand::MarkRead {
                pty_id: "missing".to_string(),
            }),
            PtyMetadataCommandResult::Missing
        );
    }
}
