#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_registry;
mod agent_sources;
mod hooks;
#[macro_use]
mod logging;
mod commands;
mod keymap;
mod layouts;
mod notifications;
mod pane_service;
mod pane_state;
mod platform;
mod pr;
mod project_service;
mod projects;
mod providers;
mod pty;
mod pty_lifecycle;
mod pty_logger;
mod pty_ready_gate;
mod services;
mod session;
mod session_service;
mod settings;
mod skill;
mod socket;
mod state;
mod tasks;
mod updater;
mod watches;
mod worktree;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
#[cfg(debug_assertions)]
use tauri_specta::{collect_commands, Builder};

use crate::pty::PtyManager;
use crate::state::AppState;

// `paths` is defined in the library crate (`roux_lib`) so both the
// `roux` and `roux-cli` binaries can use it without duplicating
// compilation — and so library visibility keeps dead-code checks from
// firing on helpers only one of the binaries calls (e.g. the legacy
// config migration only runs from `main`).
use roux_lib::paths;

fn main() {
    // Move any legacy state from the pre-unification config location
    // (`~/Library/Application Support/roux` on macOS) into the canonical
    // `~/.config/roux` before any module loads state. Best-effort; failures
    // are logged but never block startup.
    paths::migrate_legacy_config_dir();

    let initial_settings = settings::load_settings();
    logging::init(initial_settings.enable_logging);
    rlog!("Settings loaded from {:?}", paths::roux_config_dir().join("settings.json"));
    if let Some(ref p) = initial_settings.claude_binary_path {
        rlog!("Claude binary path (from settings): {}", p);
    } else {
        rlog!("Claude binary path: (default, resolved via PATH)");
    }

    let persisted_watches = watches::load_persisted_watches();
    let (watch_store_handle, _watch_join) = watches::store::spawn(persisted_watches);

    let persisted_sessions = session::load_persisted_sessions();
    let (session_handle, _session_join) = session_service::spawn(persisted_sessions);
    let (pane_handle, _pane_join) = pane_service::spawn();

    let persisted_projects = project_service::load_persisted();
    let (project_handle, _project_join) = project_service::spawn(persisted_projects);

    #[cfg(debug_assertions)]
    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::misc::get_log_path,
        commands::misc::frontend_log,
        commands::settings::get_settings,
        commands::settings::update_settings,
        commands::updater::check_for_update,
        commands::updater::install_update,
        commands::worktrees::cmd_create_worktree,
        commands::worktrees::cmd_remove_worktree,
        commands::worktrees::cmd_list_worktrees,
        commands::worktrees::cmd_preview_worktree_base,
        commands::sessions::write_to_session,
        commands::sessions::resize_session,
        commands::sessions::spawn_shell,
        commands::sessions::spawn_task,
        commands::sessions::kill_session,
        commands::sessions::restore_session,
        commands::sessions::delete_session_permanently,
        commands::sessions::session_worktree_exists,
        commands::sessions::kill_pty,
        commands::sessions::set_session_name_override,
        commands::sessions::get_pty_generation,
        commands::sessions::get_pty_cwd,
        commands::sessions::create_session_shell,
        commands::sessions::reconnect_session_shell,
        commands::sessions::list_sessions,
        commands::sessions::list_archived_sessions,
        commands::sessions::list_claude_sessions,
        commands::sessions::get_builtin_profiles,
        commands::layouts::get_builtin_layouts,
        commands::layouts::get_user_layouts,
        keymap::get_keymap,
        keymap::set_keymap,
        keymap::get_builtin_keymap_preset,
        keymap::get_keymap_path,
        commands::docs::read_file,
        commands::docs::write_file,
        commands::docs::list_docs,
        commands::misc::cmd_open_in_editor,
        commands::worktrees::cmd_list_branches,
        commands::setup::check_setup_needed,
        commands::setup::check_setup_status,
        commands::setup::run_setup,
        commands::setup::check_nono_installed,
        commands::setup::list_nono_profiles,
        commands::setup::check_doctor_status,
        commands::setup::reinstall_cli,
        commands::setup::reinstall_hooks,
        commands::setup::reinstall_skill,
        commands::setup::install_all_missing,
        commands::pr::check_gh_installed,
        commands::pr::lookup_pr,
        commands::pr::fetch_pr_branch,
        commands::pr::clone_repo,
        tasks::cmd_discover_tasks,
        tasks::cmd_load_task_overrides,
        tasks::cmd_save_task_overrides,
        commands::projects::list_projects,
        commands::projects::create_project,
        commands::projects::remove_project,
        commands::projects::rename_project,
        commands::projects::set_session_project,
        commands::notes::notes_read,
        commands::notes::notes_write,
        commands::notes::notes_append,
        commands::notes::notes_path,
        commands::notes::notes_search,
        commands::notes::notes_vault_root,
        watches::cmd_create_watch,
        watches::cmd_remove_watch,
        watches::cmd_list_watches,
        watches::cmd_pause_watch,
        watches::cmd_resume_watch,
        notifications::notifications_list,
        notifications::notifications_list_for_session,
        notifications::notifications_unread_count,
        notifications::notifications_mark_read,
        notifications::notifications_mark_all_read,
        notifications::notifications_remove,
        notifications::notifications_clear,
        notifications::notifications_push,
        notifications::notifications_dismiss_source,
        commands::sessions::check_is_git_repo,
        commands::sessions::list_git_repos_in_roots,
        commands::worktrees::git_init,
        commands::sessions::refresh_session_git_status,
        commands::misc::quit_app,
        commands::pty::list_session_ptys,
        commands::pty::detach_pty,
        commands::pty::attach_pty_to_pane,
        commands::pty::mark_pty_read,
        commands::pty::set_pty_name,
        commands::user_themes::list_user_terminal_themes,
        commands::user_themes::user_themes_dir,
        // pane_state commands are omitted from specta — serde_json::Value
        // produces invalid TypeScript. They're called via raw invoke() instead.
    ]);

    #[cfg(debug_assertions)]
    builder
        .export(specta_typescript::Typescript::default(), "../src/lib/bindings.ts")
        .expect("Failed to export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Registers the updater plugin and the `UpdaterExt` trait. The
        // `endpoints` configured in tauri.conf.json are effectively unused at
        // runtime — `src/updater.rs` overrides them per-channel via
        // `app.updater_builder().endpoints(...)`. The static config is kept as
        // a defensive fallback so checks still work if the Rust command path
        // ever regresses.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            settings: Mutex::new(initial_settings),
            pty_manager: std::sync::Arc::new(PtyManager::new()),
            pane_handle,
            session_handle,
            project_handle,
            watch_manager: watches::WatchManager::new(watch_store_handle),
            notification_manager: notifications::NotificationManager::new(),
            pending_replies: Mutex::new(std::collections::HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::misc::get_log_path,
            commands::misc::frontend_log,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::updater::check_for_update,
            commands::updater::install_update,
            commands::worktrees::cmd_create_worktree,
            commands::worktrees::cmd_remove_worktree,
            commands::worktrees::cmd_list_worktrees,
            commands::worktrees::cmd_preview_worktree_base,
            commands::sessions::write_to_session,
            commands::sessions::resize_session,
            commands::sessions::attach_pty_output,
            commands::sessions::spawn_shell,
            commands::sessions::spawn_task,
            commands::sessions::kill_session,
            commands::sessions::restore_session,
            commands::sessions::delete_session_permanently,
            commands::sessions::session_worktree_exists,
            commands::sessions::kill_pty,
            commands::sessions::set_session_name_override,
            commands::sessions::get_pty_generation,
            commands::sessions::get_pty_cwd,
            commands::sessions::create_session_shell,
            commands::sessions::reconnect_session_shell,
            commands::sessions::list_sessions,
            commands::sessions::list_archived_sessions,
            commands::sessions::list_claude_sessions,
            commands::sessions::get_builtin_profiles,
            commands::layouts::get_builtin_layouts,
            commands::layouts::get_user_layouts,
            keymap::get_keymap,
            keymap::set_keymap,
            keymap::get_builtin_keymap_preset,
            keymap::get_keymap_path,
            commands::docs::read_file,
            commands::docs::write_file,
            commands::docs::list_docs,
            commands::misc::cmd_open_in_editor,
            commands::worktrees::cmd_list_branches,
            commands::setup::check_setup_needed,
            commands::setup::check_setup_status,
            commands::setup::run_setup,
            commands::setup::check_nono_installed,
            commands::setup::list_nono_profiles,
            commands::setup::check_doctor_status,
            commands::setup::reinstall_cli,
            commands::setup::reinstall_hooks,
            commands::setup::reinstall_skill,
            commands::setup::install_all_missing,
            commands::pr::check_gh_installed,
            commands::pr::lookup_pr,
            commands::pr::fetch_pr_branch,
            tasks::cmd_discover_tasks,
            tasks::cmd_load_task_overrides,
            tasks::cmd_save_task_overrides,
            commands::projects::list_projects,
            commands::projects::create_project,
            commands::projects::remove_project,
            commands::projects::rename_project,
            commands::projects::set_session_project,
            commands::notes::notes_read,
            commands::notes::notes_write,
            commands::notes::notes_append,
            commands::notes::notes_path,
            commands::notes::notes_search,
            commands::notes::notes_vault_root,
            watches::cmd_create_watch,
            watches::cmd_remove_watch,
            watches::cmd_list_watches,
            watches::cmd_pause_watch,
            watches::cmd_resume_watch,
            notifications::notifications_list,
            notifications::notifications_list_for_session,
            notifications::notifications_unread_count,
            notifications::notifications_mark_read,
            notifications::notifications_mark_all_read,
            notifications::notifications_remove,
            notifications::notifications_clear,
            notifications::notifications_push,
            notifications::notifications_dismiss_source,
            commands::sessions::check_is_git_repo,
            commands::sessions::list_git_repos_in_roots,
            commands::worktrees::git_init,
            commands::sessions::refresh_session_git_status,
            commands::misc::quit_app,
            commands::panes::upsert_pane_record,
            commands::panes::remove_pane_record,
            commands::pane_state::load_pane_state,
            commands::pane_state::save_pane_state,
            commands::pane_state::save_live_pane_state,
            commands::pane_state::delete_pane_state,
            commands::sessions::submit_roux_reply,
            commands::pty::list_session_ptys,
            commands::pty::detach_pty,
            commands::pty::attach_pty_to_pane,
            commands::pty::mark_pty_read,
            commands::pty::set_pty_name,
        ])
        .setup(|app| {
            // Install the roux-cli shim dir (~/.config/roux/bin) with
            // `roux` and `roux-cli` symlinks so any PTY child can find them
            // without requiring a system-wide install.
            pty::ensure_roux_cli_shim();

            // Only auto-update hooks if CLI is already installed (not first run).
            // First-run install is handled by the frontend setup prompt.
            if hooks::cli_is_installed() {
                if let Err(e) = hooks::install_hooks() {
                    eprintln!("Warning: failed to install hooks: {}", e);
                }
            }
            // Agent lifecycle pipeline: a shared channel feeds AgentInput
            // values from event sources (today: FileStatusSource) into the
            // registry worker, which drives per-agent FSMs and dispatches
            // effects to the NotificationManagerSink.
            let (agent_input_tx, agent_input_rx) = std::sync::mpsc::channel();
            let sink = std::sync::Arc::new(
                agent_sources::notification_sink::NotificationManagerSink::new(
                    app.handle().clone(),
                ),
            );
            agent_registry::spawn_worker(agent_input_rx, sink);
            // Wire the PTY layer so session exits broadcast
            // `SessionEnded` into the registry — clears any stuck
            // attention notifications when Claude crashes / Ctrl-Cs
            // mid-question, since no further hook files will fire.
            {
                let state = app.state::<AppState>();
                state.pty_manager.set_agent_sender(agent_input_tx.clone());
            }

            // Spawn the PTY lifecycle handler — centralizes exit event
            // emission, PtyManager state updates, and agent registry
            // notifications in a single thread.
            {
                let state = app.state::<AppState>();
                let ctx = pty_lifecycle::LifecycleHandlerContext {
                    pty_manager: state.pty_manager.clone(),
                    agent_registry_tx: agent_input_tx.clone(),
                    app: app.handle().clone(),
                };
                let lifecycle_tx = pty_lifecycle::spawn_handler(ctx);
                state.pty_manager.set_lifecycle_tx(lifecycle_tx);
            }

            if let Err(e) = agent_sources::file_status::start_watching(
                app.handle().clone(),
                agent_input_tx,
            ) {
                eprintln!("Warning: failed to start file status source: {}", e);
            }
            socket::start_socket_server(app.handle().clone());

            // Experimental notes vault: one-shot migration of legacy
            // project notes (`~/.config/roux/notes/<id>.txt`) into the
            // new vault. Guarded by the `notes_migrated_v1` settings
            // flag so it only runs once; failures are logged, never
            // fatal.
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    run_notes_migration(app_handle).await;
                });
            }

            // Clean up orphaned watches and start active ones
            {
                let state = app.state::<AppState>();
                let session_handle = state.session_handle.clone();
                let project_handle = state.project_handle.clone();
                let app_handle = app.handle().clone();
                let watch_mgr = state.watch_manager.clone();
                tauri::async_runtime::spawn(async move {
                    let session_ids = session_handle
                        .list()
                        .await
                        .map(|s| s.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
                    let project_ids = project_handle
                        .list()
                        .await
                        .map(|p| p.iter().map(|p| p.id.clone()).collect::<Vec<_>>());

                    match (session_ids, project_ids) {
                        (Ok(sids), Ok(pids)) => {
                            let _ = watch_mgr.store().cleanup_orphans(sids, pids).await;
                        }
                        _ => {
                            eprintln!(
                                "Warning: service unavailable, skipping watch orphan cleanup"
                            );
                        }
                    }
                    watch_mgr.start_all(app_handle);
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let _ = app.emit("close-requested", ());
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // If exit was triggered programmatically (app.exit()), let it through
                if code.is_some() {
                    return;
                }
                api.prevent_exit();
                let _ = app.emit("quit-requested", ());
            }
        });
}

/// Run the one-shot legacy-notes-to-vault migration if it hasn't run yet.
/// Guarded by `settings.notes_migrated_v1`. Best-effort: failures are logged,
/// never fatal. Marks the flag as true even on partial success so the
/// migration never loops — any unmigrated files can be copied by hand.
async fn run_notes_migration(app: tauri::AppHandle) {
    let state = app.state::<crate::state::AppState>();
    let (already_done, vault_root) = match state.settings.lock() {
        Ok(s) => (
            s.notes_migrated_v1,
            s.notes_vault_root
                .clone()
                .filter(|p| !p.is_empty())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(paths::default_notes_vault_root),
        ),
        Err(_) => return,
    };
    if already_done {
        return;
    }

    let legacy = paths::roux_config_dir().join("notes");
    if !legacy.exists() {
        mark_migrated(&state);
        return;
    }

    let projects = match state.project_handle.list().await {
        Ok(ps) => ps,
        Err(e) => {
            rlog!("notes migration: project list failed: {e}");
            return;
        }
    };
    let lookup = {
        let projects = projects.clone();
        move |pid: &str| projects.iter().find(|p| p.id == pid).map(|p| p.name.clone())
    };
    let mut svc = services::notes::NotesService::new(vault_root);
    let now = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("migration-{secs}")
    };
    let migrated =
        services::notes::migrate_legacy_project_notes(&legacy, &lookup, &mut svc, &now);
    rlog!("notes migration: {migrated} file(s) moved into vault at startup");

    mark_migrated(&state);
}

fn mark_migrated(state: &tauri::State<'_, crate::state::AppState>) {
    if let Ok(mut s) = state.settings.lock() {
        s.notes_migrated_v1 = true;
        let snapshot = s.clone();
        drop(s);
        let _ = settings::save_settings(&snapshot);
    }
}
