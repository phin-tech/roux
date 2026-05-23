use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use roux_core::{Project, Session, Watch};

use crate::pane_service::{self, PaneHandle};
use crate::process_service::{self, ProcessHandle};
use crate::project_service::{self, ProjectHandle};
use crate::pty_service::{self, PtyHandle};
use crate::session_service::{self, SessionHandle};
use crate::watch_service::{self, WatchStoreHandle};

pub type RuntimeServiceFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone)]
pub struct RuntimeHost {
    pub pane_handle: PaneHandle,
    pub process_handle: ProcessHandle,
    pub pty_handle: PtyHandle,
    pub session_handle: SessionHandle,
    pub project_handle: ProjectHandle,
    pub watch_handle: WatchStoreHandle,
}

pub struct RuntimeHostConfig {
    pub initial_sessions: Vec<Session>,
    pub session_persist_path: PathBuf,
    pub initial_projects: Vec<Project>,
    pub project_persist_path: PathBuf,
    pub initial_watches: Vec<Watch>,
    pub watch_persist_path: Option<PathBuf>,
}

pub struct RuntimeHostServices {
    pub host: RuntimeHost,
    pub services: Vec<RuntimeServiceFuture>,
}

impl RuntimeHostConfig {
    pub fn build(self) -> RuntimeHostServices {
        let (session_handle, session_future) =
            session_service::service_with_path(self.initial_sessions, self.session_persist_path);
        let (pane_handle, pane_future) = pane_service::service();
        let (process_handle, process_future) = process_service::service();
        let (pty_handle, pty_future) = pty_service::service();
        let (project_handle, project_future) =
            project_service::service_with_path(self.initial_projects, self.project_persist_path);
        let (watch_handle, watch_future) =
            watch_service::service_with_path(self.initial_watches, self.watch_persist_path);

        RuntimeHostServices {
            host: RuntimeHost {
                pane_handle,
                process_handle,
                pty_handle,
                session_handle,
                project_handle,
                watch_handle,
            },
            services: vec![
                Box::pin(session_future),
                Box::pin(pane_future),
                Box::pin(process_future),
                Box::pin(pty_future),
                Box::pin(project_future),
                Box::pin(watch_future),
            ],
        }
    }
}

impl RuntimeHostServices {
    pub fn spawn_with<Join, Spawn>(self, mut spawn: Spawn) -> (RuntimeHost, Vec<Join>)
    where
        Spawn: FnMut(RuntimeServiceFuture) -> Join,
    {
        let joins = self.services.into_iter().map(&mut spawn).collect();
        (self.host, joins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_builds_six_service_futures() {
        let dir = tempfile::tempdir().unwrap();
        let services = RuntimeHostConfig {
            initial_sessions: Vec::new(),
            session_persist_path: dir.path().join("sessions.json"),
            initial_projects: Vec::new(),
            project_persist_path: dir.path().join("projects.json"),
            initial_watches: Vec::new(),
            watch_persist_path: Some(dir.path().join("watches.json")),
        }
        .build();

        assert_eq!(services.services.len(), 6);
    }
}
