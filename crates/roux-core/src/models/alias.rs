use serde::{Deserialize, Serialize};

const MAX_ALIAS_LEN: usize = 64;

/// Reserved alias names. `me` is the human-user mailbox; `human` canonicalizes
/// to `me`. The remaining names are held back for future system use and
/// rejected by `validate_user_alias_name` so user-facing CLI/socket commands
/// can't claim them.
pub const RESERVED_ALIASES: &[&str] = &["me", "human", "system", "audit", "roux"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AliasNameError {
    #[error("alias name is empty")]
    Empty,
    #[error("alias name '{0}' is too long (max 64 chars)")]
    TooLong(String),
    #[error("alias name '{0}' has invalid characters; expected lowercase letters, digits, hyphens, starting with a letter")]
    InvalidChars(String),
    #[error("alias name '{0}' is reserved")]
    Reserved(String),
}

/// Lowercase, trim, and map `human` → `me`. The store always uses canonical
/// names; lookups are case-insensitive because they go through this function.
pub fn canonical_alias_name(input: &str) -> String {
    let lower = input.trim().to_ascii_lowercase();
    if lower == "human" { "me".to_string() } else { lower }
}

/// Validate format only. Does not check reservation. Returns the canonical form.
pub fn validate_alias_name(input: &str) -> Result<String, AliasNameError> {
    let canonical = canonical_alias_name(input);
    if canonical.is_empty() {
        return Err(AliasNameError::Empty);
    }
    if canonical.len() > MAX_ALIAS_LEN {
        return Err(AliasNameError::TooLong(canonical));
    }
    let mut chars = canonical.chars();
    let first = chars.next().expect("non-empty after empty check");
    if !first.is_ascii_lowercase() {
        return Err(AliasNameError::InvalidChars(canonical));
    }
    for ch in chars {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
            return Err(AliasNameError::InvalidChars(canonical));
        }
    }
    Ok(canonical)
}

pub fn is_reserved_alias(canonical: &str) -> bool {
    RESERVED_ALIASES.contains(&canonical)
}

/// Format-validate AND reject reserved names. Use for the public CLI/socket
/// surface where user-supplied aliases shouldn't shadow system-reserved ones.
pub fn validate_user_alias_name(input: &str) -> Result<String, AliasNameError> {
    let canonical = validate_alias_name(input)?;
    if is_reserved_alias(&canonical) {
        return Err(AliasNameError::Reserved(canonical));
    }
    Ok(canonical)
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAlias {
    /// Canonical (lowercase, normalized) alias name.
    pub alias: String,
    /// Cached parent session id for grouping/filtering. Derived from the
    /// pane on bind. `None` when the alias is unbound or when bound via
    /// the legacy session-only path (Phase 1).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Canonical binding: which pane currently holds this alias.
    /// `None` for unbound aliases or for legacy entries from Phase 1
    /// (those resolve to the session's primary pane at delivery time).
    #[serde(default)]
    pub pane_id: Option<String>,
    /// Optional project scope. Used for disambiguation when the same
    /// alias name exists in multiple projects.
    #[serde(default)]
    pub project_id: Option<String>,
    /// True when the alias was auto-claimed from a pane name (vs explicit
    /// `roux alias claim`). Auto-claimed bindings release on pane rename
    /// or close; manual bindings persist for queued mail.
    #[serde(default)]
    pub auto_claimed: bool,
    /// Unix epoch milliseconds.
    pub created_at: u64,
    /// Unix epoch milliseconds. Updated on every binding change.
    pub updated_at: u64,
}

/// Tauri event emitted on every alias mutation so the frontend can update
/// without polling. Mirrors `NotificationEvent` shape.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AliasEvent {
    /// Alias was created or rebound (any binding change).
    Set { alias: AgentAlias },
    /// Alias's session binding was cleared (entry remains for queued mail).
    Unset {
        canonical: String,
        #[serde(default)]
        project_id: Option<String>,
    },
}

impl AgentAlias {
    pub fn new(alias: impl Into<String>, project_id: Option<String>) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            alias: alias.into(),
            session_id: None,
            pane_id: None,
            project_id,
            auto_claimed: false,
            created_at: now_ms,
            updated_at: now_ms,
        }
    }

    /// True when this alias has any active binding (pane-level or legacy
    /// session-level).
    pub fn is_bound(&self) -> bool {
        self.pane_id.is_some() || self.session_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_simple_lowercase() {
        assert_eq!(validate_alias_name("reviewer"), Ok("reviewer".to_string()));
        assert_eq!(validate_alias_name("frontend-team"), Ok("frontend-team".to_string()));
        assert_eq!(validate_alias_name("agent-1"), Ok("agent-1".to_string()));
    }

    #[test]
    fn validate_lowercases_uppercase_input() {
        assert_eq!(validate_alias_name("Reviewer"), Ok("reviewer".to_string()));
        assert_eq!(validate_alias_name("FRONTEND"), Ok("frontend".to_string()));
    }

    #[test]
    fn validate_trims_whitespace() {
        assert_eq!(validate_alias_name("  reviewer  "), Ok("reviewer".to_string()));
    }

    #[test]
    fn validate_rejects_empty() {
        assert_eq!(validate_alias_name(""), Err(AliasNameError::Empty));
        assert_eq!(validate_alias_name("   "), Err(AliasNameError::Empty));
    }

    #[test]
    fn validate_rejects_too_long() {
        let too_long = "a".repeat(65);
        match validate_alias_name(&too_long) {
            Err(AliasNameError::TooLong(_)) => {}
            other => panic!("expected TooLong, got {other:?}"),
        }
        let just_right = "a".repeat(64);
        assert!(validate_alias_name(&just_right).is_ok());
    }

    #[test]
    fn validate_rejects_invalid_chars() {
        for bad in ["agent_one", "agent.one", "agent one", "agent/one", "agent!"] {
            match validate_alias_name(bad) {
                Err(AliasNameError::InvalidChars(_)) => {}
                other => panic!("expected InvalidChars for {bad:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn validate_rejects_leading_digit_or_hyphen() {
        assert!(matches!(validate_alias_name("1agent"), Err(AliasNameError::InvalidChars(_))));
        assert!(matches!(validate_alias_name("-agent"), Err(AliasNameError::InvalidChars(_))));
    }

    #[test]
    fn human_canonicalizes_to_me() {
        assert_eq!(canonical_alias_name("human"), "me");
        assert_eq!(canonical_alias_name("Human"), "me");
        assert_eq!(canonical_alias_name("HUMAN"), "me");
    }

    #[test]
    fn is_reserved_covers_expected_names() {
        for name in ["me", "human", "system", "audit", "roux"] {
            assert!(
                is_reserved_alias(&canonical_alias_name(name)),
                "{name} should be reserved"
            );
        }
        assert!(!is_reserved_alias("reviewer"));
    }

    #[test]
    fn validate_user_alias_rejects_reserved() {
        assert!(matches!(validate_user_alias_name("me"), Err(AliasNameError::Reserved(_))));
        assert!(matches!(validate_user_alias_name("Human"), Err(AliasNameError::Reserved(_))));
        assert!(matches!(validate_user_alias_name("audit"), Err(AliasNameError::Reserved(_))));
    }

    #[test]
    fn validate_user_alias_accepts_normal_names() {
        assert_eq!(validate_user_alias_name("reviewer"), Ok("reviewer".to_string()));
        assert_eq!(validate_user_alias_name("Reviewer"), Ok("reviewer".to_string()));
    }

    #[test]
    fn agent_alias_new_sets_timestamps() {
        let a = AgentAlias::new("reviewer", None);
        assert_eq!(a.alias, "reviewer");
        assert!(a.session_id.is_none());
        assert!(a.created_at > 0);
        assert_eq!(a.created_at, a.updated_at);
    }
}
