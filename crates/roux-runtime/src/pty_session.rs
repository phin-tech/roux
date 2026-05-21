use roux_core::{PtyInfo, PtyRole, PtyStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExitInfo {
    pub code: Option<i32>,
    pub at_ms: u64,
    pub was_attached: bool,
}

#[derive(Debug, Clone)]
pub struct PtySessionMetadata {
    pub role: PtyRole,
    pub status: PtyStatus,
    pub exit_info: Option<PtyExitInfo>,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub working_dir: Option<String>,
    pub profile: Option<String>,
    pub last_size: (u16, u16),
    pub unread_output: bool,
    pub bell_pending: bool,
}

pub struct PtySessionMetadataInputs<'a> {
    pub role: PtyRole,
    pub pane_id: Option<&'a str>,
    pub detached_since_ms: u64,
    pub session_id: Option<&'a str>,
    pub working_dir: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub last_size: (u16, u16),
}

impl PtySessionMetadata {
    pub fn new(inputs: PtySessionMetadataInputs<'_>) -> Self {
        let status = match inputs.pane_id {
            Some(pane_id) => PtyStatus::RunningAttached { pane_id: pane_id.to_string() },
            None => PtyStatus::RunningDetached { since_ms: inputs.detached_since_ms },
        };

        Self {
            role: inputs.role,
            status,
            exit_info: None,
            session_id: inputs.session_id.map(str::to_string),
            name: None,
            working_dir: inputs.working_dir.map(str::to_string),
            profile: inputs.profile.map(str::to_string),
            last_size: inputs.last_size,
            unread_output: false,
            bell_pending: false,
        }
    }

    pub fn attach_to_pane(&mut self, pane_id: &str) {
        self.status = PtyStatus::RunningAttached { pane_id: pane_id.to_string() };
        self.unread_output = false;
        self.bell_pending = false;
    }

    pub fn detach(&mut self, since_ms: u64) {
        self.status = PtyStatus::RunningDetached { since_ms };
    }

    pub fn mark_exited(&mut self, code: Option<i32>, at_ms: u64) {
        let was_attached = matches!(self.status, PtyStatus::RunningAttached { .. });
        self.status = PtyStatus::Exited { code, at_ms };
        self.exit_info = Some(PtyExitInfo { code, at_ms, was_attached });
    }

    pub fn mark_read(&mut self) {
        self.unread_output = false;
        self.bell_pending = false;
    }

    pub fn set_unread_output(&mut self, value: bool) {
        self.unread_output = value;
    }

    pub fn set_bell_pending(&mut self, value: bool) {
        self.bell_pending = value;
    }

    pub fn set_name(&mut self, name: Option<&str>) {
        self.name = name.map(str::to_string);
    }

    pub fn belongs_to_session(&self, session_id: &str) -> bool {
        self.session_id.as_deref() == Some(session_id)
    }

    pub fn to_info(&self, id: &str) -> PtyInfo {
        PtyInfo {
            id: id.to_string(),
            session_id: self.session_id.clone(),
            role: self.role.clone(),
            status: self.status.clone(),
            name: self.name.clone(),
            working_dir: self.working_dir.clone(),
            profile: self.profile.clone(),
            unread_output: self.unread_output,
            bell_pending: self.bell_pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(pane_id: Option<&str>) -> PtySessionMetadata {
        PtySessionMetadata::new(PtySessionMetadataInputs {
            role: PtyRole::Secondary,
            pane_id,
            detached_since_ms: 1234,
            session_id: Some("session-a"),
            working_dir: Some("/repo"),
            profile: Some("plain-shell"),
            last_size: (120, 40),
        })
    }

    #[test]
    fn new_metadata_is_attached_when_pane_id_is_present() {
        let metadata = metadata(Some("pane-a"));

        assert!(matches!(
            metadata.status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-a"
        ));
        assert_eq!(metadata.session_id.as_deref(), Some("session-a"));
        assert_eq!(metadata.working_dir.as_deref(), Some("/repo"));
        assert_eq!(metadata.profile.as_deref(), Some("plain-shell"));
        assert_eq!(metadata.last_size, (120, 40));
        assert!(!metadata.unread_output);
        assert!(!metadata.bell_pending);
    }

    #[test]
    fn new_metadata_is_detached_when_pane_id_is_absent() {
        let metadata = metadata(None);

        assert!(matches!(
            metadata.status,
            PtyStatus::RunningDetached { since_ms } if since_ms == 1234
        ));
    }

    #[test]
    fn attach_detach_and_mark_read_update_flags() {
        let mut metadata = metadata(None);
        metadata.set_unread_output(true);
        metadata.set_bell_pending(true);

        metadata.attach_to_pane("pane-b");
        assert!(matches!(
            metadata.status,
            PtyStatus::RunningAttached { ref pane_id } if pane_id == "pane-b"
        ));
        assert!(!metadata.unread_output);
        assert!(!metadata.bell_pending);

        metadata.set_unread_output(true);
        metadata.set_bell_pending(true);
        metadata.detach(99);
        assert!(matches!(metadata.status, PtyStatus::RunningDetached { since_ms: 99 }));

        metadata.mark_read();
        assert!(!metadata.unread_output);
        assert!(!metadata.bell_pending);
    }

    #[test]
    fn mark_exited_records_status_and_attachment_state() {
        let mut metadata = metadata(Some("pane-a"));

        metadata.mark_exited(Some(7), 222);

        assert!(matches!(
            metadata.status,
            PtyStatus::Exited { code: Some(7), at_ms: 222 }
        ));
        assert_eq!(
            metadata.exit_info,
            Some(PtyExitInfo { code: Some(7), at_ms: 222, was_attached: true })
        );
    }

    #[test]
    fn to_info_projects_metadata_for_frontend() {
        let mut metadata = metadata(Some("pane-a"));
        metadata.set_name(Some("build shell"));
        metadata.set_unread_output(true);

        let info = metadata.to_info("pty-a");

        assert_eq!(info.id, "pty-a");
        assert_eq!(info.session_id.as_deref(), Some("session-a"));
        assert!(matches!(info.role, PtyRole::Secondary));
        assert_eq!(info.name.as_deref(), Some("build shell"));
        assert_eq!(info.working_dir.as_deref(), Some("/repo"));
        assert_eq!(info.profile.as_deref(), Some("plain-shell"));
        assert!(info.unread_output);
        assert!(!info.bell_pending);
    }

    #[test]
    fn belongs_to_session_checks_optional_session_id() {
        let metadata = metadata(Some("pane-a"));
        let detached = PtySessionMetadata::new(PtySessionMetadataInputs {
            role: PtyRole::Secondary,
            pane_id: None,
            detached_since_ms: 1,
            session_id: None,
            working_dir: None,
            profile: None,
            last_size: (80, 24),
        });

        assert!(metadata.belongs_to_session("session-a"));
        assert!(!metadata.belongs_to_session("session-b"));
        assert!(!detached.belongs_to_session("session-a"));
    }
}
