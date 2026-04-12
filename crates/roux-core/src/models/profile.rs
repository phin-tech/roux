use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Agent providers that Roux knows how to light up first-class UI for.
///
/// User-defined profiles may omit `provider` entirely (→ plain shell with
/// agent UI dark) or piggyback on an existing variant (→ misleading if the
/// agent does not actually speak the same hook protocol). A truly new agent
/// requires a provider module — see `src-tauri/src/providers/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Claude,
    Codex,
}

/// Origin of a profile in the registry. Built-in profiles are contributed by
/// provider modules at compile time; user profiles live in
/// `RouxSettings.spawn_profiles`; project profiles (reserved) will live in
/// `.roux/profiles.json` behind a workspace-trust prompt; inline profiles are
/// ad-hoc from the "Custom…" picker and never registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ProfileSource {
    Builtin,
    User,
    Project,
    Inline,
}

/// Controls whether the profile's `startup_command` runs immediately or is
/// only typed into the shell for the user to review before pressing Enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum StartupBehavior {
    AutoRun,
    TypeOnly,
}

impl Default for StartupBehavior {
    fn default() -> Self {
        Self::AutoRun
    }
}

/// A named recipe for launching something inside a shell pane. Orthogonal to
/// pane type: every launched pane is a shell, and a profile is just optional
/// metadata attached at creation describing how the shell was seeded.
///
/// Provider-specific UI (Claude Allow/Deny, resume picker) is gated on
/// observed [`crate::...`]-style runtime agent state, not on this `provider`
/// field — the field is a UX hint saying "panes launched from this profile
/// are *expected* to produce hook events from this provider".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpawnProfile {
    pub id: String,
    pub name: String,
    // Optional fields are serialized as `null` when unset rather than
    // omitted. specta's unified-mode type validator rejects
    // `skip_serializing_if` because it produces asymmetric types, and the
    // bytes saved by omission aren't worth forking serialize/deserialize.
    #[serde(default)]
    pub setup_command: Option<String>,
    #[serde(default)]
    pub startup_command: Option<String>,
    #[serde(default)]
    pub startup_behavior: Option<StartupBehavior>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub cwd_override: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub nono_profile: Option<String>,
    #[serde(default)]
    pub nono_allow_dirs: Option<Vec<String>>,
    pub source: ProfileSource,
}

impl SpawnProfile {
    /// Convenience constructor for a minimal built-in profile.
    pub fn builtin(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            setup_command: None,
            startup_command: None,
            startup_behavior: None,
            env: None,
            cwd_override: None,
            icon: None,
            provider: None,
            nono_profile: None,
            nono_allow_dirs: None,
            source: ProfileSource::Builtin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_helper_sets_source() {
        let p = SpawnProfile::builtin("plain", "Plain shell");
        assert_eq!(p.source, ProfileSource::Builtin);
        assert!(p.startup_command.is_none());
        assert!(p.provider.is_none());
    }

    #[test]
    fn json_round_trip_preserves_fields() {
        let mut p = SpawnProfile::builtin("claude", "Claude");
        p.startup_command = Some("claude --model opus".into());
        p.provider = Some(Provider::Claude);
        let json = serde_json::to_string(&p).unwrap();
        let back: SpawnProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn optional_fields_serialize_as_null_when_none() {
        // specta unified-mode rejects skip_serializing_if, so None fields
        // travel over the wire as explicit nulls. The frontend TypeScript
        // interface keeps them optional either way.
        let p = SpawnProfile::builtin("plain", "Plain shell");
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"startupCommand\":null"));
        assert!(json.contains("\"provider\":null"));
        assert!(json.contains("\"env\":null"));
    }
}
