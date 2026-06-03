use std::collections::HashMap;
use std::path::{Path, PathBuf};

use roux_core::SkillSyncMode;
use serde::{Deserialize, Serialize};

use crate::services::library::{
    list_items, LibraryItem, LibraryItemType, LibraryLayer, LibraryLayerKind,
};

const MANIFEST_FILENAME: &str = ".skill-sync.json";
const MANIFEST_VERSION: u32 = 1;

/// JSON manifest tracking which skills Roux has synced into Claude-readable
/// `.claude/skills/<name>/SKILL.md` directories. Lives at
/// `<global_root>/library/.skill-sync.json`. Used in copy mode to detect
/// "user has edited the synced file" via content hashes; symlink-mode entries
/// store no hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SkillSyncManifest {
    #[serde(default = "default_version")]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) entries: HashMap<String, SkillSyncEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillSyncEntry {
    pub(crate) skill_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_id: Option<String>,
    pub(crate) destination: PathBuf,
    pub(crate) mode: SkillSyncMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content_hash: Option<String>,
    /// Set for `Symlink` entries: the path the symlink originally pointed at.
    /// Used by unsync to verify the link still points where Roux wrote it
    /// before deleting; if the user re-pointed the symlink, the entry is
    /// reported as `KeptDueToDrift` and left alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) symlink_target: Option<PathBuf>,
    pub(crate) synced_at: String,
}

fn default_version() -> u32 {
    MANIFEST_VERSION
}

impl Default for SkillSyncManifest {
    fn default() -> Self {
        Self { version: MANIFEST_VERSION, entries: HashMap::new() }
    }
}

impl SkillSyncManifest {
    /// Stable key for an entry. Source-bound skills use the source id; skills
    /// from the global vault use the literal `global` prefix so the namespace
    /// can never collide with a user-created source id (source ids are slugged
    /// and `global:` is reserved by convention for this purpose).
    pub(crate) fn key_for(source_id: Option<&str>, skill_id: &str) -> String {
        match source_id {
            Some(sid) => format!("{sid}:{skill_id}"),
            None => format!("global:{skill_id}"),
        }
    }
}

pub(crate) fn manifest_path(global_root: &Path) -> PathBuf {
    global_root.join("library").join(MANIFEST_FILENAME)
}

/// Compute the directory under which `<skill_name>/SKILL.md` directories
/// are written for skills coming from `layer`. Returns `None` when the
/// layer cannot be resolved to a Claude-readable destination — for example
/// a `LocalRepo` layer whose root is malformed. The user-level Claude
/// directory (typically `~/.claude/skills`) is supplied by the caller so
/// this function stays pure.
///
/// Routing:
/// - `Global` and `GitRepo` → `<user_claude_skills>` (user-scoped). Git
///   sources resolve to user scope because their managed checkout lives
///   in Roux's config dir, not somewhere Claude would read from.
/// - `LocalRepo` and `ActiveRepo` → `<repo>/.claude/skills` (project-
///   scoped). Roux's library lives at `<repo>/.roux/library`, so the
///   repo root is two parents above `layer.root`.
pub(crate) fn skill_destination_dir(
    layer: &LibraryLayer,
    user_claude_skills: &Path,
) -> Option<PathBuf> {
    match layer.kind() {
        LibraryLayerKind::Global | LibraryLayerKind::GitRepo => {
            Some(user_claude_skills.to_path_buf())
        }
        LibraryLayerKind::LocalRepo | LibraryLayerKind::ActiveRepo => layer
            .root()
            .parent()
            .and_then(|p| p.parent())
            .map(|repo| repo.join(".claude").join("skills")),
    }
}

/// Path to the `SKILL.md` file for `skill_id` under `dest_dir`. Each skill
/// gets its own directory: Claude expects `<skills_root>/<name>/SKILL.md`.
pub(crate) fn skill_md_path(dest_dir: &Path, skill_id: &str) -> PathBuf {
    dest_dir.join(skill_id).join("SKILL.md")
}

pub(crate) fn compute_content_hash(body: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(body.as_bytes());
    format!("{:x}", h.finalize())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillSyncOutcome {
    Synced,
    /// Symlink was requested but the OS denied symlink creation (typical on
    /// Windows without Developer Mode); Roux fell back to copy mode for this
    /// sync. The manifest entry is recorded as `Copy` so subsequent runs
    /// detect drift correctly.
    SyncedAsCopyFallback,
    Skipped(SkillSyncSkipReason),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillSyncSkipReason {
    /// Destination matches the skill body byte-for-byte; nothing to do.
    AlreadyUpToDate,
    /// File exists at the destination but Roux has never written there
    /// before. Refuse to overwrite hand-rolled or third-party skills.
    UntrackedFile,
    /// The destination was previously written by Roux but its current
    /// content no longer matches the manifest hash — i.e. the user edited
    /// the file directly. Skip and leave it for the conflict UI.
    UserEdited,
}

pub(crate) struct SkillCopyRequest<'a> {
    pub(crate) source_id: Option<&'a str>,
    pub(crate) skill_id: &'a str,
    pub(crate) body: &'a str,
    pub(crate) destination: &'a Path,
    pub(crate) timestamp: &'a str,
}

/// Synchronize a single skill to its destination in copy mode. Updates
/// `manifest` in place when a write succeeds; the caller is responsible
/// for persisting the manifest after a batch of syncs.
pub(crate) fn sync_skill_copy(
    request: &SkillCopyRequest,
    manifest: &mut SkillSyncManifest,
) -> SkillSyncOutcome {
    let key = SkillSyncManifest::key_for(request.source_id, request.skill_id);
    let new_hash = compute_content_hash(request.body);

    if request.destination.exists() {
        let existing = match std::fs::read_to_string(request.destination) {
            Ok(s) => s,
            Err(e) => {
                return SkillSyncOutcome::Failed(format!(
                    "failed to read destination {}: {e}",
                    request.destination.display()
                ));
            }
        };
        let existing_hash = compute_content_hash(&existing);
        match manifest.entries.get(&key) {
            None => return SkillSyncOutcome::Skipped(SkillSyncSkipReason::UntrackedFile),
            Some(entry) => {
                let prior = entry.content_hash.as_deref().unwrap_or("");
                if prior != existing_hash {
                    return SkillSyncOutcome::Skipped(SkillSyncSkipReason::UserEdited);
                }
                if existing_hash == new_hash {
                    return SkillSyncOutcome::Skipped(SkillSyncSkipReason::AlreadyUpToDate);
                }
            }
        }
    }

    if let Some(parent) = request.destination.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return SkillSyncOutcome::Failed(format!(
                "failed to create destination directory {}: {e}",
                parent.display()
            ));
        }
    }
    if let Err(e) = std::fs::write(request.destination, request.body) {
        return SkillSyncOutcome::Failed(format!(
            "failed to write destination {}: {e}",
            request.destination.display()
        ));
    }

    manifest.entries.insert(
        key,
        SkillSyncEntry {
            skill_id: request.skill_id.to_string(),
            source_id: request.source_id.map(str::to_owned),
            destination: request.destination.to_path_buf(),
            mode: SkillSyncMode::Copy,
            content_hash: Some(new_hash),
            symlink_target: None,
            synced_at: request.timestamp.to_string(),
        },
    );

    SkillSyncOutcome::Synced
}

pub(crate) struct SkillSymlinkRequest<'a> {
    pub(crate) source_id: Option<&'a str>,
    pub(crate) skill_id: &'a str,
    /// Absolute path to the source `.md` file in the Library tree. The
    /// symlink will point here. Also used as the source of truth body
    /// when falling back to copy mode.
    pub(crate) source_file: &'a Path,
    pub(crate) destination: &'a Path,
    pub(crate) timestamp: &'a str,
}

/// Synchronize a single skill to its destination as a symlink. On Windows
/// without Developer Mode the OS rejects symlink creation; Roux auto-falls
/// back to copy mode for that sync and returns `SyncedAsCopyFallback` so
/// the caller can emit a one-time toast.
pub(crate) fn sync_skill_symlink(
    request: &SkillSymlinkRequest,
    manifest: &mut SkillSyncManifest,
) -> SkillSyncOutcome {
    let key = SkillSyncManifest::key_for(request.source_id, request.skill_id);

    if std::fs::symlink_metadata(request.destination).is_ok() {
        match manifest.entries.get(&key) {
            None => return SkillSyncOutcome::Skipped(SkillSyncSkipReason::UntrackedFile),
            Some(entry) => {
                if entry.mode != SkillSyncMode::Symlink {
                    // Mode change since last sync (was Copy). Caller should
                    // unsync first; refuse to silently replace.
                    return SkillSyncOutcome::Skipped(SkillSyncSkipReason::UntrackedFile);
                }
                match std::fs::read_link(request.destination) {
                    Ok(target) if target == request.source_file => {
                        return SkillSyncOutcome::Skipped(SkillSyncSkipReason::AlreadyUpToDate);
                    }
                    Ok(_) => {
                        return SkillSyncOutcome::Skipped(SkillSyncSkipReason::UserEdited);
                    }
                    Err(_) => {
                        // Was a symlink in the manifest but is no longer one.
                        return SkillSyncOutcome::Skipped(SkillSyncSkipReason::UserEdited);
                    }
                }
            }
        }
    }

    if let Some(parent) = request.destination.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return SkillSyncOutcome::Failed(format!(
                "failed to create destination directory {}: {e}",
                parent.display()
            ));
        }
    }

    match make_symlink(request.source_file, request.destination) {
        Ok(()) => {
            manifest.entries.insert(
                key,
                SkillSyncEntry {
                    skill_id: request.skill_id.to_string(),
                    source_id: request.source_id.map(str::to_owned),
                    destination: request.destination.to_path_buf(),
                    mode: SkillSyncMode::Symlink,
                    content_hash: None,
                    symlink_target: Some(request.source_file.to_path_buf()),
                    synced_at: request.timestamp.to_string(),
                },
            );
            SkillSyncOutcome::Synced
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // Windows without Developer Mode / admin: read body and copy.
            let body = match std::fs::read_to_string(request.source_file) {
                Ok(b) => b,
                Err(read_err) => {
                    return SkillSyncOutcome::Failed(format!(
                        "symlink fallback: failed to read source {}: {read_err}",
                        request.source_file.display()
                    ));
                }
            };
            let copy_req = SkillCopyRequest {
                source_id: request.source_id,
                skill_id: request.skill_id,
                body: &body,
                destination: request.destination,
                timestamp: request.timestamp,
            };
            match sync_skill_copy(&copy_req, manifest) {
                SkillSyncOutcome::Synced => SkillSyncOutcome::SyncedAsCopyFallback,
                other => other,
            }
        }
        Err(e) => SkillSyncOutcome::Failed(format!("symlink failed: {e}")),
    }
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

pub(crate) struct SkillSyncRunRequest {
    pub(crate) layers: Vec<LibraryLayer>,
    pub(crate) user_claude_skills: PathBuf,
    pub(crate) default_mode: SkillSyncMode,
    /// Per-source mode overrides keyed by `LibrarySource.id`. Sources whose
    /// id is absent inherit `default_mode`. The global vault and the active
    /// repo always use `default_mode` (they are not user-configurable
    /// sources).
    pub(crate) source_modes: HashMap<String, SkillSyncMode>,
    pub(crate) timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillSyncResult {
    pub(crate) skill_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) destination: PathBuf,
    pub(crate) requested_mode: SkillSyncMode,
    pub(crate) outcome: SkillSyncOutcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SkillSyncRunReport {
    pub(crate) results: Vec<SkillSyncResult>,
    /// Manifest entries that no longer correspond to any enabled skill —
    /// e.g. the user removed the skill from the Library or disabled the
    /// source. The orchestrator does not delete them; an explicit unsync
    /// step (Phase 4) handles removal so the user can confirm.
    pub(crate) stale: Vec<SkillSyncEntry>,
    pub(crate) symlink_fallback_count: usize,
}

/// Run a full sync pass: for each resolved skill in the layered Library,
/// compute its effective mode, destination, and apply the appropriate
/// per-skill sync. Updates `manifest` in place with successful syncs.
pub(crate) fn run_skill_sync(
    request: &SkillSyncRunRequest,
    manifest: &mut SkillSyncManifest,
) -> SkillSyncRunReport {
    let items = list_items(&request.layers);
    let mut report = SkillSyncRunReport::default();
    let mut desired_keys: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(items.len());

    for item in items {
        if item.item_type != LibraryItemType::Skill {
            continue;
        }
        let mode = effective_mode(&item, request);
        if mode == SkillSyncMode::Off {
            continue;
        }
        let Some(layer) = find_layer(&item, &request.layers) else {
            continue;
        };
        let Some(dest_dir) = skill_destination_dir(layer, &request.user_claude_skills) else {
            continue;
        };
        let dest = skill_md_path(&dest_dir, &item.id);
        let key = SkillSyncManifest::key_for(item.source_id.as_deref(), &item.id);
        desired_keys.insert(key);

        let outcome = match mode {
            SkillSyncMode::Off => continue,
            SkillSyncMode::Copy => {
                let body = match std::fs::read_to_string(&item.source_path) {
                    Ok(b) => b,
                    Err(e) => {
                        report.results.push(SkillSyncResult {
                            skill_id: item.id.clone(),
                            source_id: item.source_id.clone(),
                            destination: dest.clone(),
                            requested_mode: mode,
                            outcome: SkillSyncOutcome::Failed(format!(
                                "failed to read source {}: {e}",
                                item.source_path
                            )),
                        });
                        continue;
                    }
                };
                sync_skill_copy(
                    &SkillCopyRequest {
                        source_id: item.source_id.as_deref(),
                        skill_id: &item.id,
                        body: &body,
                        destination: &dest,
                        timestamp: &request.timestamp,
                    },
                    manifest,
                )
            }
            SkillSyncMode::Symlink => sync_skill_symlink(
                &SkillSymlinkRequest {
                    source_id: item.source_id.as_deref(),
                    skill_id: &item.id,
                    source_file: Path::new(&item.source_path),
                    destination: &dest,
                    timestamp: &request.timestamp,
                },
                manifest,
            ),
        };

        if matches!(outcome, SkillSyncOutcome::SyncedAsCopyFallback) {
            report.symlink_fallback_count += 1;
        }

        report.results.push(SkillSyncResult {
            skill_id: item.id,
            source_id: item.source_id,
            destination: dest,
            requested_mode: mode,
            outcome,
        });
    }

    for (key, entry) in &manifest.entries {
        if !desired_keys.contains(key) {
            report.stale.push(entry.clone());
        }
    }

    report
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnsyncScope {
    /// Remove every entry in the manifest.
    All,
    /// Remove entries no longer present in the desired set produced by the
    /// most recent `run_skill_sync`. Caller passes the stale list explicitly
    /// rather than re-running the orchestrator so the two stay in sync with
    /// what the UI just showed the user.
    Stale(Vec<String>),
    /// Remove entries whose `source_id` matches.
    Source(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnsyncOutcome {
    Deleted,
    /// File on disk has drifted from the manifest hash (in copy mode) or
    /// the symlink no longer points where Roux wrote it. Skipped to avoid
    /// silently deleting something the user may have customized.
    KeptDueToDrift,
    /// File was already missing — likely deleted by the user; the manifest
    /// entry is still removed.
    AlreadyGone,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnsyncResult {
    pub(crate) skill_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) destination: PathBuf,
    pub(crate) outcome: UnsyncOutcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UnsyncReport {
    pub(crate) results: Vec<UnsyncResult>,
}

/// Remove synced skill files according to `scope`. Hash-checks copy-mode
/// entries before deleting; if the file has drifted from what Roux wrote,
/// the entry is left alone (and reported as `KeptDueToDrift`). The
/// manifest is updated in place: drifted entries stay, deleted/missing
/// entries are removed.
pub(crate) fn unsync_skills(scope: &UnsyncScope, manifest: &mut SkillSyncManifest) -> UnsyncReport {
    let target_keys: Vec<String> = match scope {
        UnsyncScope::All => manifest.entries.keys().cloned().collect(),
        UnsyncScope::Stale(keys) => keys.clone(),
        UnsyncScope::Source(source_id) => manifest
            .entries
            .iter()
            .filter(|(_, e)| e.source_id.as_deref() == Some(source_id.as_str()))
            .map(|(k, _)| k.clone())
            .collect(),
    };

    let mut report = UnsyncReport::default();
    for key in target_keys {
        let Some(entry) = manifest.entries.get(&key).cloned() else {
            continue;
        };
        let outcome = unsync_one(&entry);
        let should_drop_entry =
            matches!(outcome, UnsyncOutcome::Deleted | UnsyncOutcome::AlreadyGone);
        report.results.push(UnsyncResult {
            skill_id: entry.skill_id.clone(),
            source_id: entry.source_id.clone(),
            destination: entry.destination.clone(),
            outcome,
        });
        if should_drop_entry {
            manifest.entries.remove(&key);
        }
    }
    report
}

fn unsync_one(entry: &SkillSyncEntry) -> UnsyncOutcome {
    let metadata = match std::fs::symlink_metadata(&entry.destination) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return UnsyncOutcome::AlreadyGone;
        }
        Err(e) => {
            return UnsyncOutcome::Failed(format!(
                "failed to stat {}: {e}",
                entry.destination.display()
            ));
        }
    };

    match entry.mode {
        SkillSyncMode::Off => UnsyncOutcome::Failed("manifest entry has Off mode".into()),
        SkillSyncMode::Symlink => {
            if !metadata.file_type().is_symlink() {
                return UnsyncOutcome::KeptDueToDrift;
            }
            // Verify the link still points where Roux wrote it. If the user
            // re-pointed the symlink (or the manifest predates symlink-target
            // tracking), refuse to delete.
            let Some(expected) = entry.symlink_target.as_deref() else {
                return UnsyncOutcome::KeptDueToDrift;
            };
            match std::fs::read_link(&entry.destination) {
                Ok(actual) if actual == expected => {}
                _ => return UnsyncOutcome::KeptDueToDrift,
            }
            if let Err(e) = std::fs::remove_file(&entry.destination) {
                return UnsyncOutcome::Failed(format!("remove failed: {e}"));
            }
            try_remove_empty_skill_dir(&entry.destination);
            UnsyncOutcome::Deleted
        }
        SkillSyncMode::Copy => {
            if metadata.file_type().is_symlink() {
                return UnsyncOutcome::KeptDueToDrift;
            }
            let content = match std::fs::read_to_string(&entry.destination) {
                Ok(c) => c,
                Err(e) => return UnsyncOutcome::Failed(format!("read failed: {e}")),
            };
            if entry.content_hash.as_deref() != Some(compute_content_hash(&content).as_str()) {
                return UnsyncOutcome::KeptDueToDrift;
            }
            if let Err(e) = std::fs::remove_file(&entry.destination) {
                return UnsyncOutcome::Failed(format!("remove failed: {e}"));
            }
            try_remove_empty_skill_dir(&entry.destination);
            UnsyncOutcome::Deleted
        }
    }
}

/// Remove the parent `<skills_dir>/<id>/` directory when it is empty after
/// the SKILL.md was deleted. Best-effort: any error is ignored because a
/// non-empty directory just means the user added other files we should not
/// touch.
fn try_remove_empty_skill_dir(skill_md: &Path) {
    if let Some(parent) = skill_md.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn effective_mode(item: &LibraryItem, request: &SkillSyncRunRequest) -> SkillSyncMode {
    match item.source_id.as_deref() {
        Some(id) => request.source_modes.get(id).copied().unwrap_or(request.default_mode),
        None => request.default_mode,
    }
}

fn find_layer<'a>(item: &LibraryItem, layers: &'a [LibraryLayer]) -> Option<&'a LibraryLayer> {
    layers.iter().find(|layer| {
        layer.kind() == item.source_layer && layer.source_id() == item.source_id.as_deref()
    })
}

pub(crate) fn load_manifest(global_root: &Path) -> Result<SkillSyncManifest, String> {
    let path = manifest_path(global_root);
    if !path.exists() {
        return Ok(SkillSyncManifest::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read skill sync manifest: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("invalid skill sync manifest: {e}"))
}

pub(crate) fn save_manifest(
    global_root: &Path,
    manifest: &SkillSyncManifest,
) -> Result<(), String> {
    let path = manifest_path(global_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create manifest directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("failed to serialize skill sync manifest: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write skill sync manifest: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_default_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = load_manifest(tmp.path()).unwrap();
        assert!(manifest.entries.is_empty());
        assert_eq!(manifest.version, MANIFEST_VERSION);
    }

    #[test]
    fn save_then_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut original = SkillSyncManifest::default();
        original.entries.insert(
            SkillSyncManifest::key_for(None, "rust.errors"),
            SkillSyncEntry {
                skill_id: "rust.errors".into(),
                source_id: None,
                destination: PathBuf::from("/home/user/.claude/skills/rust.errors/SKILL.md"),
                mode: SkillSyncMode::Copy,
                content_hash: Some("abc123".into()),
                symlink_target: None,
                synced_at: "2026-04-28T00:00:00Z".into(),
            },
        );
        original.entries.insert(
            SkillSyncManifest::key_for(Some("src-1"), "linked"),
            SkillSyncEntry {
                skill_id: "linked".into(),
                source_id: Some("src-1".into()),
                destination: PathBuf::from("/repo/.claude/skills/linked/SKILL.md"),
                mode: SkillSyncMode::Symlink,
                content_hash: None,
                symlink_target: Some(PathBuf::from("/vault/library/skills/linked.md")),
                synced_at: "2026-04-28T00:00:01Z".into(),
            },
        );

        save_manifest(tmp.path(), &original).unwrap();
        let loaded = load_manifest(tmp.path()).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn load_errors_on_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = manifest_path(tmp.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not valid json").unwrap();

        let err = load_manifest(tmp.path()).unwrap_err();
        assert!(err.contains("invalid skill sync manifest"), "got: {err}");
    }

    #[test]
    fn key_for_distinguishes_source_and_global() {
        assert_eq!(SkillSyncManifest::key_for(Some("src-1"), "x"), "src-1:x");
        assert_eq!(SkillSyncManifest::key_for(None, "x"), "global:x");
    }

    #[test]
    fn save_creates_parent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("does/not/exist");
        let manifest = SkillSyncManifest::default();
        save_manifest(&nested, &manifest).unwrap();
        assert!(manifest_path(&nested).exists());
    }

    fn layer(kind: LibraryLayerKind, source_id: Option<&str>, root: PathBuf) -> LibraryLayer {
        LibraryLayer::new(kind, source_id.map(str::to_owned), "test".into(), root)
    }

    #[test]
    fn destination_for_global_layer_is_user_claude_skills() {
        let user_skills = PathBuf::from("/home/user/.claude/skills");
        let l = layer(LibraryLayerKind::Global, None, PathBuf::from("/vault/library"));
        assert_eq!(skill_destination_dir(&l, &user_skills), Some(user_skills.clone()));
    }

    #[test]
    fn destination_for_git_repo_layer_is_user_claude_skills() {
        let user_skills = PathBuf::from("/home/user/.claude/skills");
        let l = layer(
            LibraryLayerKind::GitRepo,
            Some("src-1"),
            PathBuf::from("/cfg/library-sources/src-1/.roux/library"),
        );
        assert_eq!(skill_destination_dir(&l, &user_skills), Some(user_skills));
    }

    #[test]
    fn destination_for_local_repo_layer_is_repo_local_claude_skills() {
        let user_skills = PathBuf::from("/home/user/.claude/skills");
        let l = layer(
            LibraryLayerKind::LocalRepo,
            Some("src-1"),
            PathBuf::from("/work/repo/.roux/library"),
        );
        assert_eq!(
            skill_destination_dir(&l, &user_skills),
            Some(PathBuf::from("/work/repo/.claude/skills"))
        );
    }

    #[test]
    fn destination_for_active_repo_layer_is_repo_local_claude_skills() {
        let user_skills = PathBuf::from("/home/user/.claude/skills");
        let l =
            layer(LibraryLayerKind::ActiveRepo, None, PathBuf::from("/active/repo/.roux/library"));
        assert_eq!(
            skill_destination_dir(&l, &user_skills),
            Some(PathBuf::from("/active/repo/.claude/skills"))
        );
    }

    #[test]
    fn destination_returns_none_when_local_repo_root_is_too_shallow() {
        let user_skills = PathBuf::from("/home/user/.claude/skills");
        let l = layer(LibraryLayerKind::LocalRepo, Some("x"), PathBuf::from("/short"));
        assert_eq!(skill_destination_dir(&l, &user_skills), None);
    }

    #[test]
    fn skill_md_path_nests_skill_id_directory() {
        let dir = PathBuf::from("/x/.claude/skills");
        assert_eq!(
            skill_md_path(&dir, "rust.errors"),
            PathBuf::from("/x/.claude/skills/rust.errors/SKILL.md")
        );
    }

    fn copy_request<'a>(
        source_id: Option<&'a str>,
        skill_id: &'a str,
        body: &'a str,
        destination: &'a Path,
    ) -> SkillCopyRequest<'a> {
        SkillCopyRequest {
            source_id,
            skill_id,
            body,
            destination,
            timestamp: "2026-04-28T00:00:00Z",
        }
    }

    #[test]
    fn copy_first_sync_writes_file_and_records_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        let req = copy_request(None, "rust", "body\n", &dest);

        assert_eq!(sync_skill_copy(&req, &mut manifest), SkillSyncOutcome::Synced);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "body\n");
        let entry = manifest.entries.get("global:rust").unwrap();
        assert_eq!(entry.mode, SkillSyncMode::Copy);
        assert_eq!(entry.content_hash, Some(compute_content_hash("body\n")));
    }

    #[test]
    fn copy_second_sync_with_identical_body_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        let req = copy_request(None, "rust", "body\n", &dest);

        sync_skill_copy(&req, &mut manifest);
        assert_eq!(
            sync_skill_copy(&req, &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::AlreadyUpToDate)
        );
    }

    #[test]
    fn copy_skips_untracked_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "hand-rolled\n").unwrap();
        let mut manifest = SkillSyncManifest::default();
        let req = copy_request(None, "rust", "body\n", &dest);

        assert_eq!(
            sync_skill_copy(&req, &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::UntrackedFile)
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hand-rolled\n");
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn copy_skips_when_user_edited_synced_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "rust", "v1\n", &dest), &mut manifest);

        // User edits the synced file directly.
        std::fs::write(&dest, "user edit\n").unwrap();

        let req = copy_request(None, "rust", "v2\n", &dest);
        assert_eq!(
            sync_skill_copy(&req, &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::UserEdited)
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "user edit\n");
    }

    #[test]
    fn copy_overwrites_when_source_body_changes_and_dest_unmodified() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "rust", "v1\n", &dest), &mut manifest);

        let req = copy_request(None, "rust", "v2\n", &dest);
        assert_eq!(sync_skill_copy(&req, &mut manifest), SkillSyncOutcome::Synced);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "v2\n");
        assert_eq!(
            manifest.entries.get("global:rust").unwrap().content_hash,
            Some(compute_content_hash("v2\n"))
        );
    }

    #[test]
    fn copy_creates_parent_skill_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("nested/path/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        let req = copy_request(Some("src-1"), "rust", "body\n", &dest);

        assert_eq!(sync_skill_copy(&req, &mut manifest), SkillSyncOutcome::Synced);
        assert!(dest.exists());
    }

    #[cfg(unix)]
    fn write_source_skill(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}.md"));
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    #[cfg(unix)]
    fn symlink_request<'a>(
        source_id: Option<&'a str>,
        skill_id: &'a str,
        source_file: &'a Path,
        destination: &'a Path,
    ) -> SkillSymlinkRequest<'a> {
        SkillSymlinkRequest {
            source_id,
            skill_id,
            source_file,
            destination,
            timestamp: "2026-04-28T00:00:00Z",
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_first_sync_creates_link() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        let req = symlink_request(None, "rust", &source, &dest);

        assert_eq!(sync_skill_symlink(&req, &mut manifest), SkillSyncOutcome::Synced);
        let link_target = std::fs::read_link(&dest).unwrap();
        assert_eq!(link_target, source);
        let entry = manifest.entries.get("global:rust").unwrap();
        assert_eq!(entry.mode, SkillSyncMode::Symlink);
        assert_eq!(entry.content_hash, None);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_idempotent_when_link_already_points_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();

        sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest);
        assert_eq!(
            sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::AlreadyUpToDate)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_skips_untracked_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "hand-rolled\n").unwrap();
        let mut manifest = SkillSyncManifest::default();

        assert_eq!(
            sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::UntrackedFile)
        );
        assert!(manifest.entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_detects_user_replaced_link_with_file() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest);

        // User replaces the link with a regular file.
        std::fs::remove_file(&dest).unwrap();
        std::fs::write(&dest, "user content\n").unwrap();

        assert_eq!(
            sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::UserEdited)
        );
    }

    fn write_global_skill(global_root: &Path, name: &str, frontmatter: &str, body: &str) {
        let path = global_root.join("library/skills").join(format!("{name}.md"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("---\n{frontmatter}\n---\n{body}")).unwrap();
    }

    fn build_global_layer(global_root: &Path) -> LibraryLayer {
        LibraryLayer::new(
            LibraryLayerKind::Global,
            None,
            "Global".into(),
            global_root.join("library"),
        )
    }

    #[test]
    fn run_sync_off_mode_does_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        write_global_skill(
            tmp.path(),
            "rust",
            "name: rust\nid: rust\ntype: skill\ntitle: Rust",
            "Body\n",
        );
        let req = SkillSyncRunRequest {
            layers: vec![build_global_layer(tmp.path())],
            user_claude_skills: tmp.path().join("user-claude/skills"),
            default_mode: SkillSyncMode::Off,
            source_modes: HashMap::new(),
            timestamp: "ts".into(),
        };
        let mut manifest = SkillSyncManifest::default();
        let report = run_skill_sync(&req, &mut manifest);

        assert!(report.results.is_empty());
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn run_sync_copies_global_skill_to_user_claude_skills() {
        let tmp = tempfile::tempdir().unwrap();
        write_global_skill(
            tmp.path(),
            "rust",
            "name: rust\nid: rust\ntype: skill\ntitle: Rust",
            "Body\n",
        );
        let user_skills = tmp.path().join("user-claude/skills");
        let req = SkillSyncRunRequest {
            layers: vec![build_global_layer(tmp.path())],
            user_claude_skills: user_skills.clone(),
            default_mode: SkillSyncMode::Copy,
            source_modes: HashMap::new(),
            timestamp: "ts".into(),
        };
        let mut manifest = SkillSyncManifest::default();
        let report = run_skill_sync(&req, &mut manifest);

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].outcome, SkillSyncOutcome::Synced);
        let dest = user_skills.join("rust/SKILL.md");
        assert!(dest.exists());
        assert!(manifest.entries.contains_key("global:rust"));
    }

    #[test]
    fn run_sync_skips_prompts() {
        let tmp = tempfile::tempdir().unwrap();
        let prompt_path = tmp.path().join("library/prompts/p.md");
        std::fs::create_dir_all(prompt_path.parent().unwrap()).unwrap();
        std::fs::write(&prompt_path, "---\nid: p\ntype: prompt\ntitle: P\n---\nBody\n").unwrap();
        let req = SkillSyncRunRequest {
            layers: vec![build_global_layer(tmp.path())],
            user_claude_skills: tmp.path().join("user-claude/skills"),
            default_mode: SkillSyncMode::Copy,
            source_modes: HashMap::new(),
            timestamp: "ts".into(),
        };
        let mut manifest = SkillSyncManifest::default();
        let report = run_skill_sync(&req, &mut manifest);

        assert!(report.results.is_empty());
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn run_sync_marks_stale_entries_when_skill_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        write_global_skill(
            tmp.path(),
            "rust",
            "name: rust\nid: rust\ntype: skill\ntitle: Rust",
            "Body\n",
        );
        let user_skills = tmp.path().join("user-claude/skills");
        let req = SkillSyncRunRequest {
            layers: vec![build_global_layer(tmp.path())],
            user_claude_skills: user_skills.clone(),
            default_mode: SkillSyncMode::Copy,
            source_modes: HashMap::new(),
            timestamp: "ts".into(),
        };
        let mut manifest = SkillSyncManifest::default();
        run_skill_sync(&req, &mut manifest);

        // Remove the skill from the Library, run again.
        std::fs::remove_file(tmp.path().join("library/skills/rust.md")).unwrap();
        let report = run_skill_sync(&req, &mut manifest);

        assert!(report.results.is_empty());
        assert_eq!(report.stale.len(), 1);
        assert_eq!(report.stale[0].skill_id, "rust");
    }

    #[test]
    fn run_sync_per_source_mode_overrides_default() {
        let tmp = tempfile::tempdir().unwrap();
        write_global_skill(
            tmp.path(),
            "rust",
            "name: rust\nid: rust\ntype: skill\ntitle: Rust",
            "Body\n",
        );
        // Global vault skill: source_id is None, so it falls back to the default mode (Off).
        let req = SkillSyncRunRequest {
            layers: vec![build_global_layer(tmp.path())],
            user_claude_skills: tmp.path().join("user-claude/skills"),
            default_mode: SkillSyncMode::Off,
            // An override for some other source must not affect the global vault.
            source_modes: HashMap::from([("some-source".to_string(), SkillSyncMode::Copy)]),
            timestamp: "ts".into(),
        };
        let mut manifest = SkillSyncManifest::default();
        let report = run_skill_sync(&req, &mut manifest);

        assert!(report.results.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_refuses_when_existing_was_synced_as_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();

        // Prior copy-mode sync.
        sync_skill_copy(&copy_request(None, "rust", "body\n", &dest), &mut manifest);

        // Now switch the user to symlink mode without unsync first.
        assert_eq!(
            sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest),
            SkillSyncOutcome::Skipped(SkillSyncSkipReason::UntrackedFile)
        );
    }

    #[test]
    fn unsync_all_deletes_clean_copy_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "rust", "body\n", &dest), &mut manifest);

        let report = unsync_skills(&UnsyncScope::All, &mut manifest);

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].outcome, UnsyncOutcome::Deleted);
        assert!(!dest.exists());
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn unsync_keeps_copy_when_user_edited() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "rust", "body\n", &dest), &mut manifest);

        std::fs::write(&dest, "user edit\n").unwrap();

        let report = unsync_skills(&UnsyncScope::All, &mut manifest);

        assert_eq!(report.results[0].outcome, UnsyncOutcome::KeptDueToDrift);
        assert!(dest.exists());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "user edit\n");
        assert!(manifest.entries.contains_key("global:rust"));
    }

    #[test]
    fn unsync_already_gone_still_clears_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "rust", "body\n", &dest), &mut manifest);
        std::fs::remove_file(&dest).unwrap();

        let report = unsync_skills(&UnsyncScope::All, &mut manifest);

        assert_eq!(report.results[0].outcome, UnsyncOutcome::AlreadyGone);
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn unsync_source_only_removes_matching_source_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dest_a = tmp.path().join("a/SKILL.md");
        let dest_b = tmp.path().join("b/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(Some("src-1"), "a", "1\n", &dest_a), &mut manifest);
        sync_skill_copy(&copy_request(Some("src-2"), "b", "2\n", &dest_b), &mut manifest);

        let report = unsync_skills(&UnsyncScope::Source("src-1".into()), &mut manifest);

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].skill_id, "a");
        assert!(!dest_a.exists());
        assert!(dest_b.exists());
        assert!(manifest.entries.contains_key("src-2:b"));
    }

    #[test]
    fn unsync_stale_targets_only_supplied_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dest_a = tmp.path().join("a/SKILL.md");
        let dest_b = tmp.path().join("b/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "a", "1\n", &dest_a), &mut manifest);
        sync_skill_copy(&copy_request(None, "b", "2\n", &dest_b), &mut manifest);

        let report = unsync_skills(&UnsyncScope::Stale(vec!["global:a".into()]), &mut manifest);

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].skill_id, "a");
        assert!(!dest_a.exists());
        assert!(dest_b.exists());
    }

    #[cfg(unix)]
    #[test]
    fn unsync_removes_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest);

        let report = unsync_skills(&UnsyncScope::All, &mut manifest);

        assert_eq!(report.results[0].outcome, UnsyncOutcome::Deleted);
        assert!(std::fs::symlink_metadata(&dest).is_err());
        assert!(manifest.entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unsync_keeps_symlink_when_user_repointed_it() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let other = write_source_skill(&tmp.path().join("library/skills"), "other", "x\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_symlink(&symlink_request(None, "rust", &source, &dest), &mut manifest);

        // User re-points the link at a different file.
        std::fs::remove_file(&dest).unwrap();
        std::os::unix::fs::symlink(&other, &dest).unwrap();

        let report = unsync_skills(&UnsyncScope::All, &mut manifest);

        assert_eq!(report.results[0].outcome, UnsyncOutcome::KeptDueToDrift);
        assert!(std::fs::symlink_metadata(&dest).is_ok(), "user's link must remain");
        assert!(manifest.entries.contains_key("global:rust"), "manifest entry must remain");
    }

    #[cfg(unix)]
    #[test]
    fn unsync_keeps_symlink_when_manifest_predates_target_tracking() {
        // Simulates a manifest written before `symlink_target` existed —
        // the entry has Symlink mode but no recorded target. We can't
        // verify drift, so the conservative outcome is KeptDueToDrift.
        let tmp = tempfile::tempdir().unwrap();
        let source = write_source_skill(&tmp.path().join("library/skills"), "rust", "body\n");
        let dest = tmp.path().join("dest/rust/SKILL.md");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&source, &dest).unwrap();

        let mut manifest = SkillSyncManifest::default();
        manifest.entries.insert(
            "global:rust".into(),
            SkillSyncEntry {
                skill_id: "rust".into(),
                source_id: None,
                destination: dest.clone(),
                mode: SkillSyncMode::Symlink,
                content_hash: None,
                symlink_target: None,
                synced_at: "ts".into(),
            },
        );

        let report = unsync_skills(&UnsyncScope::All, &mut manifest);

        assert_eq!(report.results[0].outcome, UnsyncOutcome::KeptDueToDrift);
        assert!(std::fs::symlink_metadata(&dest).is_ok());
    }

    #[test]
    fn unsync_cleans_empty_skill_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("rust/SKILL.md");
        let mut manifest = SkillSyncManifest::default();
        sync_skill_copy(&copy_request(None, "rust", "body\n", &dest), &mut manifest);

        unsync_skills(&UnsyncScope::All, &mut manifest);

        assert!(!dest.parent().unwrap().exists(), "skill dir should be cleaned up");
    }
}
