use roux_runtime::host::RuntimeHostConfig;

use crate::{paths, platform};

pub async fn run() -> Result<(), String> {
    paths::migrate_legacy_config_dir();

    let project_path = platform::projects_path();
    let session_path = platform::sessions_path();
    let projects = roux_runtime::project_service::load_persisted_from(&project_path);
    let sessions = roux_runtime::session_service::load_persisted_from(&session_path, &projects);

    let services = RuntimeHostConfig {
        initial_sessions: sessions,
        session_persist_path: session_path,
        initial_projects: projects,
        project_persist_path: project_path,
    }
    .build();

    let (host, joins) = services.spawn_with(tokio::spawn);
    eprintln!("roux daemon started; press Ctrl-C to stop");

    tokio::signal::ctrl_c()
        .await
        .map_err(|err| format!("failed to wait for shutdown signal: {err}"))?;

    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);

    for join in joins {
        let _ = join.await;
    }

    Ok(())
}
