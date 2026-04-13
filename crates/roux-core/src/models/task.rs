use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "kebab-case")]
pub enum KeepOpen {
    Always,
    #[default]
    OnError,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub runner: String,
    pub command: String,
    pub keep_open: KeepOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskGroup {
    pub runner: String,
    pub config_file: String,
    pub tasks: Vec<TaskDefinition>,
}
