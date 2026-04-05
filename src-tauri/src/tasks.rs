use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub runner: String,
    pub command: String,
    pub keep_open: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGroup {
    pub runner: String,
    pub config_file: String,
    pub tasks: Vec<TaskDefinition>,
}

pub trait TaskDiscoverer {
    fn config_file(&self) -> &str;
    fn runner_name(&self) -> &str;
    fn discover(&self, dir: &Path) -> Option<TaskGroup>;
}

pub struct NpmDiscoverer;

impl TaskDiscoverer for NpmDiscoverer {
    fn config_file(&self) -> &str {
        "package.json"
    }

    fn runner_name(&self) -> &str {
        "npm scripts"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        let config_path = dir.join(self.config_file());
        let content = std::fs::read_to_string(&config_path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;

        let scripts = json.get("scripts")?.as_object()?;
        if scripts.is_empty() {
            return None;
        }

        let mut task_names: Vec<String> = scripts.keys().cloned().collect();
        task_names.sort();

        let tasks = task_names
            .into_iter()
            .map(|name| TaskDefinition {
                id: format!("npm:{}", name),
                name: name.clone(),
                description: String::new(),
                runner: "npm".to_string(),
                command: format!("npm run {}", name),
                keep_open: "on-error".to_string(),
            })
            .collect();

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}

pub struct TaskfileDiscoverer;

impl TaskDiscoverer for TaskfileDiscoverer {
    fn config_file(&self) -> &str {
        "Taskfile.yml"
    }

    fn runner_name(&self) -> &str {
        "Taskfile"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        let config_path = dir.join(self.config_file());
        let content = std::fs::read_to_string(&config_path).ok()?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;

        let tasks_map = yaml.get("tasks")?.as_mapping()?;
        if tasks_map.is_empty() {
            return None;
        }

        let mut task_entries: Vec<(String, String)> = tasks_map
            .iter()
            .filter_map(|(k, v)| {
                let name = k.as_str()?.to_string();
                let desc = v
                    .get("desc")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                Some((name, desc))
            })
            .collect();
        task_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let tasks = task_entries
            .into_iter()
            .map(|(name, desc)| TaskDefinition {
                id: format!("taskfile:{}", name),
                name: name.clone(),
                description: desc,
                runner: "task".to_string(),
                command: format!("task {}", name),
                keep_open: "on-error".to_string(),
            })
            .collect();

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}

pub fn discover_tasks(dir: &Path) -> Vec<TaskGroup> {
    let discoverers: Vec<Box<dyn TaskDiscoverer>> = vec![
        Box::new(NpmDiscoverer),
        Box::new(TaskfileDiscoverer),
    ];

    discoverers.into_iter().filter_map(|d| d.discover(dir)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn npm_discovers_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let package_json = serde_json::json!({
            "name": "test-project",
            "scripts": {
                "build": "tsc",
                "test": "vitest",
                "dev": "vite"
            }
        });
        fs::write(dir.path().join("package.json"), package_json.to_string()).unwrap();

        let discoverer = NpmDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();

        assert_eq!(group.runner, "npm scripts");
        assert_eq!(group.tasks.len(), 3);

        // Sorted by name: build, dev, test
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].command, "npm run build");
        assert_eq!(group.tasks[0].id, "npm:build");
        assert_eq!(group.tasks[0].keep_open, "on-error");

        assert_eq!(group.tasks[1].name, "dev");
        assert_eq!(group.tasks[2].name, "test");
    }

    #[test]
    fn npm_returns_none_without_scripts_key() {
        let dir = tempfile::tempdir().unwrap();
        let package_json = serde_json::json!({
            "name": "test-project",
            "version": "1.0.0"
        });
        fs::write(dir.path().join("package.json"), package_json.to_string()).unwrap();

        let discoverer = NpmDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }

    #[test]
    fn npm_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();

        let discoverer = NpmDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }

    #[test]
    fn discover_tasks_returns_npm_group() {
        let dir = tempfile::tempdir().unwrap();
        let package_json = serde_json::json!({
            "name": "test-project",
            "scripts": {
                "start": "node index.js"
            }
        });
        fs::write(dir.path().join("package.json"), package_json.to_string()).unwrap();

        let groups = discover_tasks(dir.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].runner, "npm scripts");
    }

    #[test]
    fn discover_tasks_returns_empty_without_configs() {
        let dir = tempfile::tempdir().unwrap();
        let groups = discover_tasks(dir.path());
        assert!(groups.is_empty());
    }

    // ---- TaskfileDiscoverer tests ----

    #[test]
    fn taskfile_discovers_tasks_with_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let taskfile = r#"
version: "3"
tasks:
  build:
    desc: Compile the project
    cmds:
      - go build ./...
  test:
    desc: Run tests
    cmds:
      - go test ./...
  lint:
    cmds:
      - golangci-lint run
"#;
        fs::write(dir.path().join("Taskfile.yml"), taskfile).unwrap();

        let discoverer = TaskfileDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();

        assert_eq!(group.runner, "Taskfile");
        assert_eq!(group.config_file, "Taskfile.yml");
        assert_eq!(group.tasks.len(), 3);

        // Sorted: build, lint, test
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].description, "Compile the project");
        assert_eq!(group.tasks[0].command, "task build");
        assert_eq!(group.tasks[0].id, "taskfile:build");
        assert_eq!(group.tasks[0].keep_open, "on-error");

        assert_eq!(group.tasks[1].name, "lint");
        assert_eq!(group.tasks[1].description, "");

        assert_eq!(group.tasks[2].name, "test");
        assert_eq!(group.tasks[2].description, "Run tests");
    }

    #[test]
    fn taskfile_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        let discoverer = TaskfileDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }
}
