#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hooks;
#[macro_use]
mod logging;
mod commands;
mod notifications;
mod pane_state;
mod projects;
mod project_service;
mod services;
mod pty;
mod session;
mod session_service;
mod settings;
mod socket;
mod state;
mod status_watcher;
mod tasks;
mod watches;
mod worktree;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_specta::{Builder, collect_commands};

use crate::pty::PtyManager;
use crate::state::AppState;

fn main() {
    let initial_settings = settings::load_settings();
    logging::init(initial_settings.enable_logging);
    rlog!("Settings loaded from {:?}", dirs::config_dir().map(|d| d.join("roux/settings.json")));
    if let Some(ref p) = initial_settings.claude_binary_path {
        rlog!("Claude binary path (from settings): {}", p);
    } else {
        rlog!("Claude binary path: (default, resolved via PATH)");
    }

    let persisted_watches = watches::load_persisted_watches();
    let (watch_store_handle, _watch_join) = watches::store::spawn(persisted_watches);

    let persisted_sessions = session::load_persisted_sessions();
    let (session_handle, _session_join) = session_service::spawn(persisted_sessions);

    let persisted_projects = project_service::load_persisted();
    let (project_handle, _project_join) = project_service::spawn(persisted_projects);

    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::misc::get_log_path,
            commands::misc::frontend_log,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::worktrees::cmd_create_worktree,
            commands::worktrees::cmd_remove_worktree,
            commands::worktrees::cmd_list_worktrees,
            commands::sessions::write_to_session,
            commands::sessions::resize_session,
            commands::sessions::spawn_shell,
            commands::sessions::spawn_task,
            commands::sessions::kill_session,
            commands::sessions::get_pty_generation,
            commands::sessions::get_pty_cwd,
            commands::sessions::create_session,
            commands::sessions::reconnect_session,
            commands::sessions::list_sessions,
            commands::sessions::list_claude_sessions,
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
            tasks::cmd_discover_tasks,
            tasks::cmd_load_task_overrides,
            tasks::cmd_save_task_overrides,
            commands::projects::list_projects,
            commands::projects::create_project,
            commands::projects::remove_project,
            commands::projects::rename_project,
            commands::projects::set_session_project,
            commands::projects::get_project_notes,
            commands::projects::set_project_notes,
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
            commands::worktrees::git_init,
            commands::sessions::refresh_session_git_status,
            commands::misc::quit_app,
            // pane_state commands are omitted from specta — serde_json::Value
            // produces invalid TypeScript. They're called via raw invoke() instead.
        ]);

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/lib/bindings.ts",
        )
        .expect("Failed to export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            settings: Mutex::new(initial_settings),
            pty_manager: PtyManager::new(),
            session_handle,
            project_handle,
            watch_manager: watches::WatchManager::new(watch_store_handle),
            notification_manager: notifications::NotificationManager::new(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::misc::get_log_path,
            commands::misc::frontend_log,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::worktrees::cmd_create_worktree,
            commands::worktrees::cmd_remove_worktree,
            commands::worktrees::cmd_list_worktrees,
            commands::sessions::write_to_session,
            commands::sessions::resize_session,
            commands::sessions::attach_pty_output,
            commands::sessions::spawn_shell,
            commands::sessions::spawn_task,
            commands::sessions::kill_session,
            commands::sessions::get_pty_generation,
            commands::sessions::get_pty_cwd,
            commands::sessions::create_session,
            commands::sessions::reconnect_session,
            commands::sessions::list_sessions,
            commands::sessions::list_claude_sessions,
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
            tasks::cmd_discover_tasks,
            tasks::cmd_load_task_overrides,
            tasks::cmd_save_task_overrides,
            commands::projects::list_projects,
            commands::projects::create_project,
            commands::projects::remove_project,
            commands::projects::rename_project,
            commands::projects::set_session_project,
            commands::projects::get_project_notes,
            commands::projects::set_project_notes,
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
            commands::worktrees::git_init,
            commands::sessions::refresh_session_git_status,
            commands::misc::quit_app,
            commands::pane_state::load_pane_state,
            commands::pane_state::save_pane_state,
            commands::pane_state::delete_pane_state,
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
            if let Err(e) = status_watcher::start_watching(app.handle().clone()) {
                eprintln!("Warning: failed to start status watcher: {}", e);
            }
            socket::start_socket_server(app.handle().clone());

            // Clean up orphaned watches and start active ones
            {
                let state = app.state::<AppState>();
                let session_handle = state.session_handle.clone();
                let project_handle = state.project_handle.clone();
                let app_handle = app.handle().clone();
                let watch_mgr = state.watch_manager.clone();
                tauri::async_runtime::spawn(async move {
                    let session_ids = session_handle.list().await
                        .map(|s| s.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
                    let project_ids = project_handle.list().await
                        .map(|p| p.iter().map(|p| p.id.clone()).collect::<Vec<_>>());

                    match (session_ids, project_ids) {
                        (Ok(sids), Ok(pids)) => {
                            let _ = watch_mgr.store().cleanup_orphans(sids, pids).await;
                        }
                        _ => {
                            eprintln!("Warning: service unavailable, skipping watch orphan cleanup");
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
