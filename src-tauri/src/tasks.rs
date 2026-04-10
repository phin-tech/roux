use std::collections::HashMap;
use std::path::Path;

pub use roux_core::{TaskDefinition, TaskGroup};

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
                keep_open: roux_core::KeepOpen::OnError,
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
                let desc = v.get("desc").and_then(|d| d.as_str()).unwrap_or("").to_string();
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
                keep_open: roux_core::KeepOpen::OnError,
            })
            .collect();

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}

pub struct MakeDiscoverer;

impl TaskDiscoverer for MakeDiscoverer {
    fn config_file(&self) -> &str {
        "Makefile"
    }

    fn runner_name(&self) -> &str {
        "Makefile"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        let config_path = dir.join(self.config_file());
        let content = std::fs::read_to_string(&config_path).ok()?;

        let lines: Vec<&str> = content.lines().collect();
        let mut task_entries: Vec<(String, String)> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            // Must contain `:` and not be indented
            let colon_pos = match line.find(':') {
                Some(pos) => pos,
                None => continue,
            };

            let target = &line[..colon_pos];

            // Skip if target is empty
            if target.is_empty() {
                continue;
            }

            // Skip targets starting with `.` or `_`
            if target.starts_with('.') || target.starts_with('_') {
                continue;
            }

            // Skip if target contains `$`, `%`, or spaces
            if target.contains('$') || target.contains('%') || target.contains(' ') {
                continue;
            }

            // Target must be valid identifier-like characters only
            if !target.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                continue;
            }

            // Look for `## comment` on the immediately preceding line
            let desc = if i > 0 {
                let prev = lines[i - 1].trim();
                if let Some(stripped) = prev.strip_prefix("##") {
                    stripped.trim().to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            task_entries.push((target.to_string(), desc));
        }

        if task_entries.is_empty() {
            return None;
        }

        task_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let tasks = task_entries
            .into_iter()
            .map(|(name, desc)| TaskDefinition {
                id: format!("make:{}", name),
                name: name.clone(),
                description: desc,
                runner: "make".to_string(),
                command: format!("make {}", name),
                keep_open: roux_core::KeepOpen::OnError,
            })
            .collect();

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}

pub struct JustDiscoverer;

impl TaskDiscoverer for JustDiscoverer {
    fn config_file(&self) -> &str {
        "justfile"
    }

    fn runner_name(&self) -> &str {
        "Justfile"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        // Case-insensitive: try `justfile` then `Justfile`
        let config_path = if dir.join("justfile").exists() {
            dir.join("justfile")
        } else if dir.join("Justfile").exists() {
            dir.join("Justfile")
        } else {
            return None;
        };

        let content = std::fs::read_to_string(&config_path).ok()?;
        let lines: Vec<&str> = content.lines().collect();
        let mut task_entries: Vec<(String, String)> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            // Skip empty lines and comment lines
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Recipe lines are non-indented (no leading whitespace) and contain `:`
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }

            let colon_pos = match line.find(':') {
                Some(pos) => pos,
                None => continue,
            };

            let before_colon = &line[..colon_pos];
            // First word before colon is the recipe name
            let first_word = before_colon.split_whitespace().next().unwrap_or("");
            if first_word.is_empty() {
                continue;
            }

            // Strip leading `@` from silent recipes
            let name = first_word.trim_start_matches('@').to_string();
            if name.is_empty() {
                continue;
            }

            // Skip lines that look like variable assignments (name contains `=`)
            if name.contains('=') {
                continue;
            }

            // Description from `# comment` on preceding line
            let desc = if i > 0 {
                let prev = lines[i - 1].trim();
                if let Some(stripped) = prev.strip_prefix('#') {
                    stripped.trim().to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            task_entries.push((name, desc));
        }

        if task_entries.is_empty() {
            return None;
        }

        task_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let tasks = task_entries
            .into_iter()
            .map(|(name, desc)| TaskDefinition {
                id: format!("just:{}", name),
                name: name.clone(),
                description: desc,
                runner: "just".to_string(),
                command: format!("just {}", name),
                keep_open: roux_core::KeepOpen::OnError,
            })
            .collect();

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: "justfile".to_string(),
            tasks,
        })
    }
}

pub fn discover_tasks(dir: &Path) -> Vec<TaskGroup> {
    let discoverers: Vec<Box<dyn TaskDiscoverer>> = vec![
        Box::new(NpmDiscoverer),
        Box::new(TaskfileDiscoverer),
        Box::new(MakeDiscoverer),
        Box::new(JustDiscoverer),
    ];

    discoverers.into_iter().filter_map(|d| d.discover(dir)).collect()
}

fn overrides_path() -> std::path::PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("roux").join("task-overrides.json")
}

#[tauri::command]
#[specta::specta]
pub fn cmd_discover_tasks(dir: String) -> Vec<TaskGroup> {
    discover_tasks(Path::new(&dir))
}

#[tauri::command]
#[specta::specta]
pub fn cmd_load_task_overrides() -> HashMap<String, HashMap<String, String>> {
    let path = overrides_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

#[tauri::command]
#[specta::specta]
pub fn cmd_save_task_overrides(
    overrides: HashMap<String, HashMap<String, String>>,
) -> Result<(), String> {
    let path = overrides_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&overrides).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
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
        assert_eq!(group.tasks[0].keep_open, roux_core::KeepOpen::OnError);

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
        assert_eq!(group.tasks[0].keep_open, roux_core::KeepOpen::OnError);

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

    // ---- MakeDiscoverer tests ----

    #[test]
    fn make_discovers_targets_with_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let makefile = r#".PHONY: build test lint

## Build the project
build:
	go build ./...

## Run tests
test:
	go test ./...

lint:
	golangci-lint run

_internal:
	echo "skip me"
"#;
        fs::write(dir.path().join("Makefile"), makefile).unwrap();

        let discoverer = MakeDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();

        assert_eq!(group.runner, "Makefile");
        assert_eq!(group.config_file, "Makefile");

        // .PHONY and _internal should be skipped; build, test, lint remain
        assert_eq!(group.tasks.len(), 3);

        // Sorted: build, lint, test
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].description, "Build the project");
        assert_eq!(group.tasks[0].command, "make build");
        assert_eq!(group.tasks[0].id, "make:build");
        assert_eq!(group.tasks[0].keep_open, roux_core::KeepOpen::OnError);

        assert_eq!(group.tasks[1].name, "lint");
        assert_eq!(group.tasks[1].description, "");

        assert_eq!(group.tasks[2].name, "test");
        assert_eq!(group.tasks[2].description, "Run tests");
    }

    #[test]
    fn make_skips_dot_and_underscore_targets() {
        let dir = tempfile::tempdir().unwrap();
        let makefile = r#".PHONY: all
_helper:
	echo helper
all:
	echo all
"#;
        fs::write(dir.path().join("Makefile"), makefile).unwrap();

        let discoverer = MakeDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.tasks.len(), 1);
        assert_eq!(group.tasks[0].name, "all");
    }

    #[test]
    fn make_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        let discoverer = MakeDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }

    // ---- JustDiscoverer tests ----

    #[test]
    fn just_discovers_recipes_with_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let justfile = r#"# Build the project
build:
    cargo build

# Run tests
test:
    cargo test

lint:
    cargo clippy
"#;
        fs::write(dir.path().join("justfile"), justfile).unwrap();

        let discoverer = JustDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();

        assert_eq!(group.runner, "Justfile");
        assert_eq!(group.config_file, "justfile");
        assert_eq!(group.tasks.len(), 3);

        // Sorted: build, lint, test
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].description, "Build the project");
        assert_eq!(group.tasks[0].command, "just build");
        assert_eq!(group.tasks[0].id, "just:build");
        assert_eq!(group.tasks[0].keep_open, roux_core::KeepOpen::OnError);

        assert_eq!(group.tasks[1].name, "lint");
        assert_eq!(group.tasks[1].description, "");

        assert_eq!(group.tasks[2].name, "test");
        assert_eq!(group.tasks[2].description, "Run tests");
    }

    #[test]
    fn just_discovers_case_insensitive_justfile() {
        let dir = tempfile::tempdir().unwrap();
        let justfile = r#"build:
    cargo build
"#;
        // Write with capital J
        fs::write(dir.path().join("Justfile"), justfile).unwrap();

        let discoverer = JustDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.tasks.len(), 1);
        assert_eq!(group.tasks[0].name, "build");
    }

    #[test]
    fn just_strips_at_sign_from_silent_recipes() {
        let dir = tempfile::tempdir().unwrap();
        let justfile = r#"@build:
    cargo build
"#;
        fs::write(dir.path().join("justfile"), justfile).unwrap();

        let discoverer = JustDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.tasks.len(), 1);
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].command, "just build");
    }

    #[test]
    fn just_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        let discoverer = JustDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }

    // ---- Integration test ----

    #[test]
    fn discover_tasks_finds_multiple_runners() {
        let dir = tempfile::tempdir().unwrap();

        // Write package.json with scripts
        let package_json = serde_json::json!({
            "name": "test-project",
            "scripts": {
                "start": "node index.js"
            }
        });
        fs::write(dir.path().join("package.json"), package_json.to_string()).unwrap();

        // Write Makefile with a target
        let makefile = r#"## Start the server
start:
	node index.js
"#;
        fs::write(dir.path().join("Makefile"), makefile).unwrap();

        let groups = discover_tasks(dir.path());
        assert_eq!(groups.len(), 2);

        let runners: Vec<&str> = groups.iter().map(|g| g.runner.as_str()).collect();
        assert!(runners.contains(&"npm scripts"));
        assert!(runners.contains(&"Makefile"));
    }
}
