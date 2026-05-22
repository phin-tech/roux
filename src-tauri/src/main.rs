#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_registry;
mod agent_sources;
mod automation_hooks;
mod hooks;
#[macro_use]
mod logging;
mod commands;
mod daemon_client;
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
mod tray;
mod updater;
mod watches;
mod worktree;

use std::sync::Mutex;
use tauri::{Emitter, Listener, Manager};
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
    pty::set_shell_binary_path_override(initial_settings.shell_binary_path.clone());
    logging::init(initial_settings.enable_logging);
    rlog!("Settings loaded from {:?}", paths::roux_config_dir().join("settings.json"));
    if let Some(ref p) = initial_settings.claude_binary_path {
        rlog!("Claude binary path (from settings): {}", p);
    } else {
        rlog!("Claude binary path: (default, resolved via PATH)");
    }

    let persisted_watches = watches::load_persisted_watches();
    let (watch_store_handle, _watch_join) = watches::store::spawn(persisted_watches);
    let daemon_client = daemon_client::DaemonClient::detect();
    if let Some(client) = daemon_client.as_ref() {
        rlog!(
            "Connected to roux daemon pid={} socket={}",
            client.status().pid,
            client.status().socket
        );
    } else {
        rlog!("No roux daemon detected; desktop will self-host runtime state");
    }

    let persisted_projects = project_service::load_persisted();
    let persisted_sessions = session::load_persisted_sessions(&persisted_projects);
    let runtime_services = roux_runtime::host::RuntimeHostConfig {
        initial_sessions: persisted_sessions,
        session_persist_path: session::persistence_path(),
        initial_projects: persisted_projects,
        project_persist_path: paths::roux_config_dir().join("projects.json"),
    }
    .build();
    let (runtime, _runtime_joins) = runtime_services.spawn_with(tauri::async_runtime::spawn);

    #[cfg(debug_assertions)]
    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::misc::get_log_path,
        commands::misc::frontend_log,
        commands::settings::get_settings,
        commands::settings::update_settings,
        commands::mcp::cmd_mcp_status,
        commands::mcp::cmd_preview_mcp_host_config,
        commands::mcp::cmd_configure_mcp_host,
        commands::updater::check_for_update,
        commands::updater::install_update,
        commands::worktrees::cmd_create_worktree,
        commands::worktrees::cmd_remove_worktree,
        commands::worktrees::cmd_list_worktrees,
        commands::worktrees::cmd_preview_worktree_base,
        commands::worktrees::cmd_detect_worktrunk,
        commands::worktrees::cmd_worktrunk_diagnostics,
        commands::worktrees::cmd_worktrunk_read_log,
        commands::worktrees::cmd_open_terminal_at,
        commands::worktrees::cmd_open_path_in_finder,
        commands::smol_machines::cmd_detect_smolvm,
        commands::smol_machines::cmd_list_smol_machines,
        commands::smol_machines::cmd_start_smol_machine,
        commands::smol_machines::cmd_stop_smol_machine,
        commands::smol_machines::cmd_delete_smol_machine,
        commands::smol_machines::cmd_create_smol_machine,
        commands::smol_machines::cmd_check_smolvm_binary,
        commands::smol_machines::cmd_install_smolvm_agent,
        commands::smol_machines::cmd_install_smolvm_agent_persist,
        commands::smol_machines::cmd_install_smolvm_agent_recreate,
        commands::smol_machines::cmd_list_smol_machine_smolfiles,
        commands::smol_machines::cmd_open_smolvm_bootstrap_config,
        commands::smol_machines::cmd_start_managed_proxy,
        commands::smol_machines::cmd_stop_managed_proxy,
        commands::smol_machines::cmd_managed_proxy_status,
        commands::smol_machines::cmd_check_worktree_mount,
        commands::smol_machines::cmd_append_worktree_mount,
        automation_hooks::cmd_list_automation_hooks,
        automation_hooks::cmd_preview_automation_hooks,
        automation_hooks::cmd_run_automation_hook,
        automation_hooks::cmd_approve_automation_hook,
        automation_hooks::cmd_clear_automation_hook_approvals,
        automation_hooks::cmd_list_automation_hook_logs,
        automation_hooks::cmd_read_automation_hook_log,
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
        commands::sessions::set_session_pinned_pr_url,
        commands::sessions::set_session_smol_machine,
        commands::sessions::refresh_session_branch,
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
        commands::setup::cmd_agent_notification_setup_status,
        commands::setup::cmd_preview_codex_notification_config,
        commands::setup::cmd_configure_codex_notification_config,
        commands::setup::run_setup,
        commands::setup::check_nono_installed,
        commands::setup::list_nono_profiles,
        commands::setup::check_doctor_status,
        commands::setup::reinstall_cli,
        commands::setup::reinstall_hooks,
        commands::setup::reinstall_skill,
        commands::setup::install_all_missing,
        commands::setup::cmd_detect_gh,
        commands::setup::cmd_detect_git,
        commands::pr::check_gh_installed,
        commands::pr::lookup_pr,
        commands::pr::fetch_pr_branch,
        commands::pr::clone_repo,
        commands::pr::lookup_pr_for_branch,
        tasks::cmd_discover_tasks,
        tasks::cmd_load_task_overrides,
        tasks::cmd_save_task_overrides,
        commands::projects::list_projects,
        commands::projects::create_project,
        commands::projects::remove_project,
        commands::projects::rename_project,
        commands::projects::update_project,
        commands::projects::set_session_project,
        commands::notes::notes_read,
        commands::notes::notes_write,
        commands::notes::notes_append,
        commands::notes::notes_path,
        commands::notes::notes_search,
        commands::notes::notes_vault_root,
        watches::cmd_create_watch,
        watches::cmd_find_or_create_watch,
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
        commands::pty::list_all_ptys,
        commands::pty::detach_pty,
        commands::pty::attach_pty_to_pane,
        commands::pty::mark_pty_read,
        commands::pty::set_pty_name,
        commands::user_themes::list_user_terminal_themes,
        commands::user_themes::user_themes_dir,
        // pane_state commands are omitted from specta — serde_json::Value
        // produces invalid TypeScript. They're called via raw invoke() instead.
        // Subscription commands are registered with `invoke_handler!`
        // only (below). They're hand-typed in `src/lib/types/mailbox.ts`
        // to mirror the mailbox/aliases pattern.
    ]);

    #[cfg(debug_assertions)]
    builder
        .export(specta_typescript::Typescript::default(), "../src/lib/bindings.ts")
        .expect("Failed to export TypeScript bindings");

    // Subscription manager loaded once and shared by both the
    // MailboxManager (so post() can fan out matches) and AppState (so
    // the Tauri/CLI/MCP layers can mutate the subscription set). Clone
    // is cheap — internally an Arc.
    let subscription_manager = roux_lib::subscriptions::SubscriptionManager::load();

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
            daemon_client,
            pty_manager: std::sync::Arc::new(PtyManager::new()),
            runtime,
            watch_manager: watches::WatchManager::new(watch_store_handle),
            automation_hooks: automation_hooks::AutomationHookManager::new(),
            notification_manager: notifications::NotificationManager::new(),
            alias_manager: roux_lib::aliases::AliasManager::load(),
            // Subscriptions feed into the mailbox manager so topic events
            // become visible / ack-able to subscribers. The same handle
            // also lives on AppState so the Tauri/CLI/MCP layers can
            // mutate the subscription set; clone is cheap (Arc inside).
            mailbox_manager: roux_lib::mailbox::MailboxManager::load()
                .with_subscriptions(subscription_manager.clone()),
            subscription_manager,
            pending_replies: Mutex::new(std::collections::HashMap::new()),
            managed_proxy: services::managed_proxy::ManagedProxyState::new(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::misc::get_log_path,
            commands::misc::frontend_log,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::mcp::cmd_mcp_status,
            commands::mcp::cmd_preview_mcp_host_config,
            commands::mcp::cmd_configure_mcp_host,
            commands::updater::check_for_update,
            commands::updater::install_update,
            commands::worktrees::cmd_create_worktree,
            commands::worktrees::cmd_remove_worktree,
            commands::worktrees::cmd_list_worktrees,
            commands::worktrees::cmd_preview_worktree_base,
            commands::worktrees::cmd_detect_worktrunk,
            commands::worktrees::cmd_worktrunk_diagnostics,
            commands::worktrees::cmd_worktrunk_read_log,
            commands::worktrees::cmd_open_terminal_at,
            commands::worktrees::cmd_open_path_in_finder,
            commands::smol_machines::cmd_detect_smolvm,
            commands::smol_machines::cmd_list_smol_machines,
            commands::smol_machines::cmd_start_smol_machine,
            commands::smol_machines::cmd_stop_smol_machine,
            commands::smol_machines::cmd_delete_smol_machine,
            commands::smol_machines::cmd_create_smol_machine,
            commands::smol_machines::cmd_check_smolvm_binary,
            commands::smol_machines::cmd_install_smolvm_agent,
            commands::smol_machines::cmd_install_smolvm_agent_persist,
            commands::smol_machines::cmd_install_smolvm_agent_recreate,
            commands::smol_machines::cmd_list_smol_machine_smolfiles,
            commands::smol_machines::cmd_open_smolvm_bootstrap_config,
            commands::smol_machines::cmd_start_managed_proxy,
            commands::smol_machines::cmd_stop_managed_proxy,
            commands::smol_machines::cmd_managed_proxy_status,
            commands::smol_machines::cmd_check_worktree_mount,
            commands::smol_machines::cmd_append_worktree_mount,
            automation_hooks::cmd_list_automation_hooks,
            automation_hooks::cmd_preview_automation_hooks,
            automation_hooks::cmd_run_automation_hook,
            automation_hooks::cmd_approve_automation_hook,
            automation_hooks::cmd_clear_automation_hook_approvals,
            automation_hooks::cmd_list_automation_hook_logs,
            automation_hooks::cmd_read_automation_hook_log,
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
            commands::sessions::set_session_pinned_pr_url,
            commands::sessions::set_session_smol_machine,
            commands::sessions::refresh_session_branch,
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
            commands::library::list_library_items,
            commands::library::read_library_item,
            commands::library::render_library_prompt,
            commands::library::save_library_item,
            commands::library::get_library_pinned_repos,
            commands::library::set_library_pinned_repos,
            commands::library::list_library_sources,
            commands::library::set_library_sources,
            commands::library::clone_library_source,
            commands::library::sync_library_source,
            commands::library::get_library_source_status,
            commands::library::get_library_source_statuses,
            commands::library_sync::library_skill_sync_run,
            commands::library_sync::library_skill_sync_unsync,
            commands::misc::cmd_open_in_editor,
            commands::worktrees::cmd_list_branches,
            commands::setup::check_setup_needed,
            commands::setup::check_setup_status,
            commands::setup::cmd_agent_notification_setup_status,
            commands::setup::cmd_preview_codex_notification_config,
            commands::setup::cmd_configure_codex_notification_config,
            commands::setup::run_setup,
            commands::setup::check_nono_installed,
            commands::setup::list_nono_profiles,
            commands::setup::check_doctor_status,
            commands::setup::reinstall_cli,
            commands::setup::reinstall_hooks,
            commands::setup::reinstall_skill,
            commands::setup::install_all_missing,
            commands::setup::cmd_detect_gh,
            commands::setup::cmd_detect_git,
            commands::pr::check_gh_installed,
            commands::pr::lookup_pr,
            commands::pr::fetch_pr_branch,
            commands::pr::lookup_pr_for_branch,
            tasks::cmd_discover_tasks,
            tasks::cmd_load_task_overrides,
            tasks::cmd_save_task_overrides,
            commands::projects::list_projects,
            commands::projects::create_project,
            commands::projects::remove_project,
            commands::projects::rename_project,
            commands::projects::update_project,
            commands::projects::set_session_project,
            commands::projects::render_project_prompt_template,
            commands::notes::notes_read,
            commands::notes::notes_write,
            commands::notes::notes_append,
            commands::notes::notes_path,
            commands::notes::notes_search,
            commands::notes::notes_vault_root,
            watches::cmd_create_watch,
            watches::cmd_find_or_create_watch,
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
            commands::pty::list_all_ptys,
            commands::pty::detach_pty,
            commands::pty::attach_pty_to_pane,
            commands::pty::mark_pty_read,
            commands::pty::set_pty_name,
            commands::user_themes::list_user_terminal_themes,
            commands::user_themes::user_themes_dir,
            commands::aliases::aliases_list,
            commands::aliases::aliases_get,
            commands::aliases::aliases_whoami,
            commands::aliases::aliases_add_member,
            commands::aliases::aliases_remove_member,
            commands::aliases::aliases_set_mode,
            commands::mailbox::mailbox_list_for_recipient,
            commands::mailbox::mailbox_list_for_topic,
            commands::mailbox::mailbox_list_all,
            commands::mailbox::mailbox_unread_count,
            commands::mailbox::mailbox_get_event,
            commands::mailbox::mailbox_read_state,
            commands::mailbox::mailbox_post,
            commands::mailbox::mailbox_mark_read,
            commands::mailbox::mailbox_ack,
            commands::mailbox::mailbox_clear_read,
            commands::mailbox::mailbox_retract,
            commands::mailbox::mailbox_dismiss,
            commands::mailbox::mailbox_deliver_to_pane,
            commands::subscriptions::subscriptions_list,
            commands::subscriptions::subscriptions_create,
            commands::subscriptions::subscriptions_delete,
        ])
        .setup(|app| {
            // Install the Roux CLI shim dir (~/.config/roux/bin) with
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
                    automation_hooks: state.automation_hooks.clone(),
                    app: app.handle().clone(),
                };
                let lifecycle_tx = pty_lifecycle::spawn_handler(ctx);
                state.pty_manager.set_lifecycle_tx(lifecycle_tx);
            }

            if let Err(e) =
                agent_sources::file_status::start_watching(app.handle().clone(), agent_input_tx)
            {
                eprintln!("Warning: failed to start file status source: {}", e);
            }
            {
                let state = app.state::<AppState>();
                if state.daemon_client.is_some() {
                    rlog!("Skipping desktop socket server because roux daemon owns the socket");
                } else {
                    socket::start_socket_server(app.handle().clone());
                }
            }

            // System tray: shows active sessions + status, plus Show/Quit.
            // Failure here is non-fatal (e.g. headless CI); log and continue.
            if let Err(e) = tray::setup(app.handle()) {
                eprintln!("Warning: failed to set up tray: {}", e);
            }

            // Refresh the tray menu when any session's status changes.
            // `tray::refresh` is a cheap signal — the worker started in
            // `tray::setup` does the actual work.
            app.listen("roux-status-update", |_event| {
                tray::refresh();
            });

            // Refresh the tray menu when notifications change (added,
            // read, removed, cleared). Surfaces the unread count and
            // the recent-unread submenu without polling.
            app.listen(notifications::NOTIFICATION_EVENT, |_event| {
                tray::refresh();
            });

            // Low-frequency poll catches session add/remove/archive
            // (no dedicated event bus for those today). 3s is fine —
            // the refresh worker coalesces overlapping signals.
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3));
                ticker.tick().await; // skip the immediate first tick
                loop {
                    ticker.tick().await;
                    tray::refresh();
                }
            });

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

            // Library skills: one-shot rewrite to SKILL.md-compatible
            // format (adds `name:`, strips legacy `variables:` blocks).
            // Guarded by `library_skill_format_v2_migrated`. Failures
            // are logged per-file, never fatal.
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    run_library_skill_migration(app_handle).await;
                });
            }

            // Clean up orphaned watches and start active ones
            {
                let state = app.state::<AppState>();
                let session_handle = state.runtime.session_handle.clone();
                let project_handle = state.runtime.project_handle.clone();
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

    let projects = match state.runtime.project_handle.list().await {
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
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        format!("migration-{secs}")
    };
    let migrated = services::notes::migrate_legacy_project_notes(&legacy, &lookup, &mut svc, &now);
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

/// Run the one-shot global-Library skill format migration if it hasn't
/// run yet. Guarded by `settings.library_skill_format_v2_migrated`.
/// Best-effort: per-file failures are logged but the flag is still set
/// so the migration never loops. Repo and git-source Library skills are
/// migrated implicitly the next time the user saves them.
async fn run_library_skill_migration(app: tauri::AppHandle) {
    let state = app.state::<crate::state::AppState>();
    let (already_done, vault_root) = match state.settings.lock() {
        Ok(s) => (
            s.library_skill_format_v2_migrated,
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

    let report = services::library::migrate_global_skills(&vault_root);
    if !report.migrated.is_empty() {
        rlog!("library skill migration: rewrote {} file(s)", report.migrated.len());
    }
    for (path, err) in &report.errors {
        rlog!("library skill migration: {} failed: {err}", path.display());
    }

    mark_library_skill_migrated(&state);
}

fn mark_library_skill_migrated(state: &tauri::State<'_, crate::state::AppState>) {
    if let Ok(mut s) = state.settings.lock() {
        s.library_skill_format_v2_migrated = true;
        let snapshot = s.clone();
        drop(s);
        let _ = settings::save_settings(&snapshot);
    }
}
