use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::platform;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
}

pub struct ProjectStore {
    projects: Arc<Mutex<Vec<Project>>>,
    dirty: Arc<AtomicBool>,
}

impl ProjectStore {
    pub fn load_persisted() -> Self {
        let path = Self::persistence_path();
        let projects = if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        let store = Self {
            projects: Arc::new(Mutex::new(projects)),
            dirty: Arc::new(AtomicBool::new(false)),
        };
        store.start_persist_thread();
        store
    }

    fn start_persist_thread(&self) {
        let projects = Arc::clone(&self.projects);
        let dirty = Arc::clone(&self.dirty);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if dirty.swap(false, Ordering::AcqRel) {
                let snapshot = {
                    let guard = projects.lock().unwrap();
                    guard.clone()
                };
                Self::write_to_disk(&snapshot);
            }
        });
    }

    pub fn add(&self, project: Project) {
        let mut projects = self.projects.lock().unwrap();
        projects.push(project);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn remove(&self, id: &str) {
        let mut projects = self.projects.lock().unwrap();
        projects.retain(|p| p.id != id);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn rename(&self, id: &str, name: &str) {
        let mut projects = self.projects.lock().unwrap();
        if let Some(p) = projects.iter_mut().find(|p| p.id == id) {
            p.name = name.to_string();
        }
        self.dirty.store(true, Ordering::Release);
    }

    pub fn list(&self) -> Vec<Project> {
        self.projects.lock().unwrap().clone()
    }

    fn persistence_path() -> PathBuf {
        platform::projects_path()
    }

    fn write_to_disk(projects: &[Project]) {
        let path = Self::persistence_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(projects) {
            let _ = fs::write(&path, json);
        }
    }
}
