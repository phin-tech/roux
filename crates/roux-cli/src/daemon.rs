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

    wait_for_shutdown_signal().await?;

    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);

    for join in joins {
        if let Err(err) = join.await {
            return Err(format!("daemon task join failed: {err}"));
        }
    }

    Ok(())
}

async fn wait_for_shutdown_signal() -> Result<(), String> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|err| format!("failed to install SIGTERM handler: {err}"))?;

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|err| format!("failed to wait for SIGINT: {err}"))?;
            }
            _ = sigterm.recv() => {}
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|err| format!("failed to wait for shutdown signal: {err}"))
    }
}
