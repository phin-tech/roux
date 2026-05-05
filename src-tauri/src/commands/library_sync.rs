use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use roux_core::SkillSyncMode;
use serde::{Deserialize, Serialize};

use crate::paths::{default_notes_vault_root, user_claude_skills_dir};
use crate::services::library as svc;
use crate::services::library_sync as sync;
use crate::state::AppState;

fn join_err(e: tauri::Error) -> String {
    format!("task join: {e}")
}

/// Process-global lock around the skill-sync manifest's read-modify-write
/// cycle. Both `library_skill_sync_run` and `library_skill_sync_unsync`
/// load → mutate → save the same manifest file; without this, two
/// overlapping invocations can each load stale state and the later save
/// silently drops the earlier command's updates (or resurrects entries
/// that were just unsynced). Held only on the blocking thread; never
/// across an `.await`.
fn manifest_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Tag identifying the variant of `SkillSyncOutcome` for the frontend.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SkillSyncOutcomeKind {
    Synced,
    SyncedAsCopyFallback,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SkillSyncSkipReasonDto {
    AlreadyUpToDate,
    UntrackedFile,
    UserEdited,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillSyncResultDto {
    pub(crate) skill_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) destination: String,
    pub(crate) requested_mode: SkillSyncMode,
    pub(crate) outcome: SkillSyncOutcomeKind,
    pub(crate) skip_reason: Option<SkillSyncSkipReasonDto>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillSyncEntryDto {
    pub(crate) skill_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) destination: String,
    pub(crate) mode: SkillSyncMode,
    pub(crate) synced_at: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillSyncRunReportDto {
    pub(crate) results: Vec<SkillSyncResultDto>,
    pub(crate) stale: Vec<SkillSyncEntryDto>,
    pub(crate) symlink_fallback_count: u32,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub(crate) enum UnsyncScopeDto {
    All,
    Stale(Vec<String>),
    Source(String),
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UnsyncOutcomeKind {
    Deleted,
    KeptDueToDrift,
    AlreadyGone,
    Failed,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnsyncResultDto {
    pub(crate) skill_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) destination: String,
    pub(crate) outcome: UnsyncOutcomeKind,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnsyncReportDto {
    pub(crate) results: Vec<UnsyncResultDto>,
}

fn now_unix_secs() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}

fn managed_sources_root() -> PathBuf {
    crate::paths::roux_config_dir().join("library-sources")
}

async fn active_repo_for_session(state: &AppState, session_id: Option<&str>) -> Option<String> {
    let id = session_id?;
    state.session_handle.get(id).await.ok().flatten().map(|session| session.repo_root)
}

fn outcome_to_dto(outcome: &sync::SkillSyncOutcome) -> (SkillSyncOutcomeKind, Option<SkillSyncSkipReasonDto>, Option<String>) {
    match outcome {
        sync::SkillSyncOutcome::Synced => (SkillSyncOutcomeKind::Synced, None, None),
        sync::SkillSyncOutcome::SyncedAsCopyFallback => {
            (SkillSyncOutcomeKind::SyncedAsCopyFallback, None, None)
        }
        sync::SkillSyncOutcome::Skipped(reason) => {
            let r = match reason {
                sync::SkillSyncSkipReason::AlreadyUpToDate => SkillSyncSkipReasonDto::AlreadyUpToDate,
                sync::SkillSyncSkipReason::UntrackedFile => SkillSyncSkipReasonDto::UntrackedFile,
                sync::SkillSyncSkipReason::UserEdited => SkillSyncSkipReasonDto::UserEdited,
            };
            (SkillSyncOutcomeKind::Skipped, Some(r), None)
        }
        sync::SkillSyncOutcome::Failed(msg) => {
            (SkillSyncOutcomeKind::Failed, None, Some(msg.clone()))
        }
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn library_skill_sync_run(
    session_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<SkillSyncRunReportDto, String> {
    let active_repo = active_repo_for_session(&state, session_id.as_deref()).await;
    let settings =
        state.settings.lock().map_err(|_| "settings lock poisoned".to_string())?.clone();
    let global_root = settings
        .notes_vault_root
        .as_ref()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_vault_root);
    let managed_root = managed_sources_root();
    let library_sources = settings.library_sources.clone();
    let default_mode = settings.library_skill_sync_default;
    let user_skills_dir = user_claude_skills_dir();
    let timestamp = now_unix_secs();

    tauri::async_runtime::spawn_blocking(move || {
        let layers = svc::layers(
            global_root.clone(),
            &library_sources,
            &managed_root,
            active_repo,
        );
        let source_modes: HashMap<String, SkillSyncMode> = library_sources
            .iter()
            .filter(|s| s.enabled)
            .filter_map(|s| s.skill_sync.map(|mode| (s.id.clone(), mode)))
            .collect();

        // Hold the manifest lock across the load → mutate → save cycle so
        // an overlapping `library_skill_sync_unsync` (or another sync_run)
        // can't read stale state and overwrite us.
        let _manifest_guard = manifest_guard();

        let mut manifest =
            sync::load_manifest(&global_root).map_err(|e| format!("load manifest: {e}"))?;

        let request = sync::SkillSyncRunRequest {
            layers,
            user_claude_skills: user_skills_dir,
            default_mode,
            source_modes,
            timestamp,
        };
        let report = sync::run_skill_sync(&request, &mut manifest);

        sync::save_manifest(&global_root, &manifest).map_err(|e| format!("save manifest: {e}"))?;

        Ok::<SkillSyncRunReportDto, String>(SkillSyncRunReportDto {
            results: report
                .results
                .iter()
                .map(|r| {
                    let (outcome, skip_reason, error) = outcome_to_dto(&r.outcome);
                    SkillSyncResultDto {
                        skill_id: r.skill_id.clone(),
                        source_id: r.source_id.clone(),
                        destination: r.destination.to_string_lossy().into_owned(),
                        requested_mode: r.requested_mode,
                        outcome,
                        skip_reason,
                        error,
                    }
                })
                .collect(),
            stale: report
                .stale
                .iter()
                .map(|e| SkillSyncEntryDto {
                    skill_id: e.skill_id.clone(),
                    source_id: e.source_id.clone(),
                    destination: e.destination.to_string_lossy().into_owned(),
                    mode: e.mode,
                    synced_at: e.synced_at.clone(),
                })
                .collect(),
            symlink_fallback_count: report.symlink_fallback_count as u32,
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn library_skill_sync_unsync(
    scope: UnsyncScopeDto,
    state: tauri::State<'_, AppState>,
) -> Result<UnsyncReportDto, String> {
    let settings =
        state.settings.lock().map_err(|_| "settings lock poisoned".to_string())?.clone();
    let global_root = settings
        .notes_vault_root
        .as_ref()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_vault_root);

    tauri::async_runtime::spawn_blocking(move || {
        // Same lock as `library_skill_sync_run` — load → mutate → save
        // must serialize against any concurrent sync to avoid resurrecting
        // unsynced entries or dropping freshly-synced ones.
        let _manifest_guard = manifest_guard();

        let mut manifest =
            sync::load_manifest(&global_root).map_err(|e| format!("load manifest: {e}"))?;

        let engine_scope = match scope {
            UnsyncScopeDto::All => sync::UnsyncScope::All,
            UnsyncScopeDto::Stale(keys) => sync::UnsyncScope::Stale(keys),
            UnsyncScopeDto::Source(id) => sync::UnsyncScope::Source(id),
        };
        let report = sync::unsync_skills(&engine_scope, &mut manifest);

        sync::save_manifest(&global_root, &manifest).map_err(|e| format!("save manifest: {e}"))?;

        Ok::<UnsyncReportDto, String>(UnsyncReportDto {
            results: report
                .results
                .iter()
                .map(|r| {
                    let (kind, error) = match &r.outcome {
                        sync::UnsyncOutcome::Deleted => (UnsyncOutcomeKind::Deleted, None),
                        sync::UnsyncOutcome::KeptDueToDrift => (UnsyncOutcomeKind::KeptDueToDrift, None),
                        sync::UnsyncOutcome::AlreadyGone => (UnsyncOutcomeKind::AlreadyGone, None),
                        sync::UnsyncOutcome::Failed(msg) => {
                            (UnsyncOutcomeKind::Failed, Some(msg.clone()))
                        }
                    };
                    UnsyncResultDto {
                        skill_id: r.skill_id.clone(),
                        source_id: r.source_id.clone(),
                        destination: r.destination.to_string_lossy().into_owned(),
                        outcome: kind,
                        error,
                    }
                })
                .collect(),
        })
    })
    .await
    .map_err(join_err)?
}
