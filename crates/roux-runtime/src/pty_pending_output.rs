use std::collections::HashMap;

pub struct PtyPendingOutput<Channel> {
    pending: HashMap<String, Channel>,
}

impl<Channel> PtyPendingOutput<Channel> {
    pub fn new() -> Self {
        Self { pending: HashMap::new() }
    }

    pub fn insert(&mut self, pty_id: String, channel: Channel) -> Option<Channel> {
        self.pending.insert(pty_id, channel)
    }

    pub fn remove(&mut self, pty_id: &str) -> Option<Channel> {
        self.pending.remove(pty_id)
    }

    pub fn retain_existing(&mut self, mut exists: impl FnMut(&str) -> bool) {
        self.pending.retain(|pty_id, _| exists(pty_id));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    fn contains_key(&self, pty_id: &str) -> bool {
        self.pending.contains_key(pty_id)
    }
}

impl<Channel> Default for PtyPendingOutput<Channel> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_remove_round_trip_pending_channel() {
        let mut pending = PtyPendingOutput::new();

        assert_eq!(pending.insert("pty-a".to_string(), vec![1, 2, 3]), None);
        assert_eq!(pending.remove("pty-a"), Some(vec![1, 2, 3]));
        assert_eq!(pending.remove("pty-a"), None);
    }

    #[test]
    fn insert_replaces_existing_channel_for_same_pty() {
        let mut pending = PtyPendingOutput::new();

        assert_eq!(pending.insert("pty-a".to_string(), "first"), None);
        assert_eq!(pending.insert("pty-a".to_string(), "second"), Some("first"));
        assert_eq!(pending.remove("pty-a"), Some("second"));
    }

    #[test]
    fn retain_existing_drops_entries_without_live_sessions() {
        let mut pending = PtyPendingOutput::new();
        pending.insert("pty-a".to_string(), 1);
        pending.insert("pty-b".to_string(), 2);
        pending.insert("pty-c".to_string(), 3);

        pending.retain_existing(|pty_id| pty_id == "pty-a" || pty_id == "pty-c");

        assert_eq!(pending.len(), 2);
        assert!(pending.contains_key("pty-a"));
        assert!(!pending.contains_key("pty-b"));
        assert!(pending.contains_key("pty-c"));
    }
}
