use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use roux_core::{LibrarySource, LibrarySourceKind};
use roux_git::{GitCli, RemoteState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LibraryItemType {
    Prompt,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LibraryLayerKind {
    Global,
    LocalRepo,
    GitRepo,
    ActiveRepo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LibraryRemoteState {
    UpToDate,
    Ahead,
    Behind,
    Diverged,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LibraryVariableType {
    #[default]
    String,
    Int,
    Float,
    Select,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryVariable {
    pub(crate) name: String,
    pub(crate) label: Option<String>,
    pub(crate) default: Option<String>,
    pub(crate) required: bool,
    #[serde(default)]
    pub(crate) value_type: LibraryVariableType,
    #[serde(default)]
    pub(crate) options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryItem {
    pub(crate) id: String,
    pub(crate) item_type: LibraryItemType,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) provider: Option<String>,
    pub(crate) source_layer: LibraryLayerKind,
    pub(crate) source_id: Option<String>,
    pub(crate) source_label: String,
    pub(crate) source_path: String,
    pub(crate) overridden_paths: Vec<String>,
    pub(crate) variables: Vec<LibraryVariable>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryRead {
    pub(crate) item: LibraryItem,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderLibraryPromptRequest {
    pub(crate) item_id: String,
    pub(crate) variables: HashMap<String, String>,
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderedLibraryPrompt {
    pub(crate) item_id: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveLibraryItemRequest {
    pub(crate) original_id: Option<String>,
    pub(crate) item_id: String,
    pub(crate) item_type: LibraryItemType,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) provider: Option<String>,
    pub(crate) variables: Vec<LibraryVariable>,
    pub(crate) body: String,
    pub(crate) target: SaveLibraryTarget,
    pub(crate) expected_source_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type", content = "id")]
pub(crate) enum SaveLibraryTarget {
    Global,
    Source(String),
    ActiveRepo,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedLibraryItem {
    pub(crate) item_id: String,
    pub(crate) source_path: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryGitStatus {
    pub(crate) source_id: String,
    pub(crate) checked_out: bool,
    pub(crate) checkout_path: String,
    pub(crate) branch: String,
    pub(crate) tracking_branch: Option<String>,
    pub(crate) default_branch: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) remote_state: LibraryRemoteState,
    pub(crate) ahead: u32,
    pub(crate) behind: u32,
    pub(crate) behind_default: Option<u32>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryLayer {
    kind: LibraryLayerKind,
    source_id: Option<String>,
    label: String,
    root: PathBuf,
}

#[derive(Debug, Clone)]
struct ParsedItem {
    item: LibraryItem,
    body: String,
}

pub(crate) fn layers(
    global_root: PathBuf,
    sources: &[LibrarySource],
    managed_sources_root: &Path,
    active_repo: Option<String>,
) -> Vec<LibraryLayer> {
    let mut out = Vec::new();
    out.push(LibraryLayer {
        kind: LibraryLayerKind::Global,
        source_id: None,
        label: "Global".into(),
        root: global_root.join("library"),
    });
    for source in sources.iter().filter(|source| source.enabled) {
        match source.kind {
            LibrarySourceKind::LocalRepo => {
                let Some(path) = source.path.as_ref().map(|p| p.trim()).filter(|p| !p.is_empty())
                else {
                    continue;
                };
                out.push(LibraryLayer {
                    kind: LibraryLayerKind::LocalRepo,
                    source_id: Some(source.id.clone()),
                    label: source.name.clone(),
                    root: Path::new(path).join(".roux").join("library"),
                });
            }
            LibrarySourceKind::GitRepo => {
                out.push(LibraryLayer {
                    kind: LibraryLayerKind::GitRepo,
                    source_id: Some(source.id.clone()),
                    label: source.name.clone(),
                    root: checkout_path_for_source(managed_sources_root, source)
                        .join(".roux")
                        .join("library"),
                });
            }
        }
    }
    if let Some(repo) = active_repo {
        let trimmed = repo.trim();
        if !trimmed.is_empty() {
            out.push(LibraryLayer {
                kind: LibraryLayerKind::ActiveRepo,
                source_id: None,
                label: "Active repo".into(),
                root: Path::new(trimmed).join(".roux").join("library"),
            });
        }
    }
    out
}

pub(crate) fn list_items(layers: &[LibraryLayer]) -> Vec<LibraryItem> {
    let mut resolved: HashMap<String, ParsedItem> = HashMap::new();
    let mut order = Vec::<String>::new();

    for layer in layers {
        for parsed in scan_layer(layer) {
            let id = parsed.item.id.clone();
            let mut next = parsed;
            if let Some(existing) = resolved.get(&id) {
                next.item.overridden_paths.push(existing.item.source_path.clone());
                next.item.overridden_paths.extend(existing.item.overridden_paths.clone());
            } else {
                order.push(id.clone());
            }
            resolved.insert(id, next);
        }
    }

    order.into_iter().filter_map(|id| resolved.remove(&id).map(|p| p.item)).collect()
}

pub(crate) fn read_item(layers: &[LibraryLayer], item_id: &str) -> Result<LibraryRead, String> {
    let parsed = resolve_item(layers, item_id)
        .ok_or_else(|| format!("library item not found: {item_id}"))?;
    Ok(LibraryRead { item: parsed.item, body: parsed.body })
}

pub(crate) fn render_prompt(
    layers: &[LibraryLayer],
    request: RenderLibraryPromptRequest,
) -> Result<RenderedLibraryPrompt, String> {
    let read = read_item(layers, &request.item_id)?;
    if read.item.item_type != LibraryItemType::Prompt {
        return Err(format!("library item is not a prompt: {}", request.item_id));
    }
    let mut values = HashMap::new();
    for variable in &read.item.variables {
        if let Some(value) = request.variables.get(&variable.name) {
            validate_variable_value(variable, value)?;
            values.insert(variable.name.clone(), value.clone());
        } else if let Some(default) = &variable.default {
            validate_variable_value(variable, default)?;
            values.insert(variable.name.clone(), default.clone());
        } else if variable.required {
            return Err(format!("missing required variable: {}", variable.name));
        }
    }
    Ok(RenderedLibraryPrompt {
        item_id: request.item_id,
        content: render_template(&read.body, &values),
    })
}

pub(crate) fn save_item(
    global_root: PathBuf,
    sources: &[LibrarySource],
    managed_sources_root: &Path,
    active_repo: Option<String>,
    request: SaveLibraryItemRequest,
) -> Result<SavedLibraryItem, String> {
    let item_id = request.item_id.trim();
    if !is_valid_item_id(item_id) {
        return Err(
            "library item id must contain only letters, numbers, dots, underscores, or hyphens"
                .to_string(),
        );
    }
    if let Some(original_id) =
        request.original_id.as_ref().map(|id| id.trim()).filter(|id| !id.is_empty())
    {
        if !is_valid_item_id(original_id) {
            return Err("original library item id is invalid".to_string());
        }
    }
    let title = request.title.trim();
    if title.is_empty() {
        return Err("library item title is required".to_string());
    }
    let root = target_library_root(
        global_root.clone(),
        sources,
        managed_sources_root,
        active_repo.clone(),
        &request.target,
    )?;
    let dir_name = match request.item_type {
        LibraryItemType::Prompt => "prompts",
        LibraryItemType::Skill => "skills",
    };
    let next_path = root.join(dir_name).join(format!("{}.md", file_stem_for_id(item_id)));
    let original_path = request
        .original_id
        .as_ref()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .and_then(|id| {
            read_item(&layers(global_root, sources, managed_sources_root, active_repo), id).ok()
        })
        .map(|parsed| PathBuf::from(parsed.item.source_path));
    if let Some(original_path) = original_path.as_ref() {
        if let Some(expected) =
            request.expected_source_path.as_ref().filter(|path| !path.trim().is_empty())
        {
            if Path::new(expected) != original_path.as_path() {
                return Err("source file changed on disk; reload before saving".to_string());
            }
        }
        if original_path.as_path() != next_path.as_path() && next_path.exists() {
            return Err(format!("target file already exists: {}", next_path.display()));
        }
        if !original_path.exists() {
            return Err("source file changed on disk; reload before saving".to_string());
        }
    } else if request.original_id.as_ref().is_some_and(|id| !id.trim().is_empty()) {
        return Err("source file changed on disk; reload before saving".to_string());
    } else if next_path.exists() {
        return Err(format!("target file already exists: {}", next_path.display()));
    }
    if let Some(parent) = next_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create library directory: {e}"))?;
    }
    let markdown = serialize_item_markdown(&request, item_id, title)?;
    std::fs::write(&next_path, markdown)
        .map_err(|e| format!("failed to save library item: {e}"))?;
    if let Some(original_path) = original_path.as_ref().filter(|path| *path != &next_path) {
        std::fs::remove_file(original_path)
            .map_err(|e| format!("failed to move old library item file: {e}"))?;
    }
    Ok(SavedLibraryItem {
        item_id: item_id.to_string(),
        source_path: next_path.to_string_lossy().into_owned(),
    })
}

pub(crate) fn checkout_path_for_source(
    managed_sources_root: &Path,
    source: &LibrarySource,
) -> PathBuf {
    managed_sources_root.join(safe_source_dir(&source.id))
}

pub(crate) fn clone_git_source(
    managed_sources_root: &Path,
    source: &LibrarySource,
) -> Result<String, String> {
    if source.kind != LibrarySourceKind::GitRepo {
        return Err("library source is not a git source".to_string());
    }
    let url = source
        .url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| "git library source is missing a URL".to_string())?;
    let checkout_path = checkout_path_for_source(managed_sources_root, source);
    crate::services::setup::git_cli()
        .clone_repo(url, source.branch.as_deref(), &checkout_path)
        .map_err(|e| e.to_string())?;
    Ok(checkout_path.to_string_lossy().into_owned())
}

pub(crate) fn sync_git_source(
    managed_sources_root: &Path,
    source: &LibrarySource,
) -> Result<LibraryGitStatus, String> {
    if source.kind != LibrarySourceKind::GitRepo {
        return Err("library source is not a git source".to_string());
    }
    let checkout_path = checkout_path_for_source(managed_sources_root, source);
    let git = crate::services::setup::git_cli();
    if !git.is_repo(&checkout_path) {
        return Err("git library source has not been cloned yet".to_string());
    }
    let before = git_status(managed_sources_root, source);
    if before.dirty {
        return Err(
            "git library source has uncommitted changes; commit or discard them before syncing"
                .to_string(),
        );
    }
    if before.remote_state == LibraryRemoteState::Diverged {
        return Err(
            "git library source has diverged from its remote; resolve it manually before syncing"
                .to_string(),
        );
    }
    git.sync_branch(&checkout_path, source.branch.as_deref()).map_err(|e| e.to_string())?;
    Ok(git_status(managed_sources_root, source))
}

pub(crate) fn git_status(managed_sources_root: &Path, source: &LibrarySource) -> LibraryGitStatus {
    let git = crate::services::setup::git_cli();
    git_status_with_git(managed_sources_root, source, &git)
}

pub(crate) fn git_status_with_git(
    managed_sources_root: &Path,
    source: &LibrarySource,
    git: &GitCli,
) -> LibraryGitStatus {
    let checkout_path = checkout_path_for_source(managed_sources_root, source);
    let checkout_path_string = checkout_path.to_string_lossy().into_owned();
    if source.kind != LibrarySourceKind::GitRepo {
        return LibraryGitStatus {
            source_id: source.id.clone(),
            checked_out: false,
            checkout_path: checkout_path_string,
            branch: source.branch.clone().unwrap_or_default(),
            tracking_branch: None,
            default_branch: None,
            dirty: false,
            remote_state: LibraryRemoteState::Unknown,
            ahead: 0,
            behind: 0,
            behind_default: None,
            error: Some("library source is not a git source".to_string()),
        };
    }
    if !checkout_path.exists() {
        return LibraryGitStatus {
            source_id: source.id.clone(),
            checked_out: false,
            checkout_path: checkout_path_string,
            branch: source.branch.clone().unwrap_or_default(),
            tracking_branch: None,
            default_branch: None,
            dirty: false,
            remote_state: LibraryRemoteState::Unknown,
            ahead: 0,
            behind: 0,
            behind_default: None,
            error: None,
        };
    }
    if !git.is_repo(&checkout_path) {
        return LibraryGitStatus {
            source_id: source.id.clone(),
            checked_out: false,
            checkout_path: checkout_path_string,
            branch: source.branch.clone().unwrap_or_default(),
            tracking_branch: None,
            default_branch: None,
            dirty: false,
            remote_state: LibraryRemoteState::Unknown,
            ahead: 0,
            behind: 0,
            behind_default: None,
            error: Some("checkout path is not a git repo".to_string()),
        };
    }

    let status = match git.status(&checkout_path) {
        Ok(status) => status,
        Err(e) => {
            return LibraryGitStatus {
                source_id: source.id.clone(),
                checked_out: true,
                checkout_path: checkout_path_string,
                branch: source.branch.clone().unwrap_or_default(),
                tracking_branch: None,
                default_branch: None,
                dirty: false,
                remote_state: LibraryRemoteState::Unknown,
                ahead: 0,
                behind: 0,
                behind_default: None,
                error: Some(e.to_string()),
            }
        }
    };

    LibraryGitStatus {
        source_id: source.id.clone(),
        checked_out: true,
        checkout_path: checkout_path_string,
        branch: status
            .branch
            .or_else(|| source.branch.clone())
            .unwrap_or_else(|| "HEAD".to_string()),
        tracking_branch: status.tracking_branch,
        default_branch: status.default_branch,
        dirty: status.dirty,
        remote_state: remote_state_from_git(status.remote_state),
        ahead: status.ahead,
        behind: status.behind,
        behind_default: status.behind_default,
        error: None,
    }
}

fn resolve_item(layers: &[LibraryLayer], item_id: &str) -> Option<ParsedItem> {
    let mut found = None;
    for layer in layers {
        for parsed in scan_layer(layer) {
            if parsed.item.id == item_id {
                found = Some(parsed);
            }
        }
    }
    found
}

fn scan_layer(layer: &LibraryLayer) -> Vec<ParsedItem> {
    let mut items = Vec::new();
    scan_kind(layer, LibraryItemType::Prompt, "prompts", &mut items);
    scan_kind(layer, LibraryItemType::Skill, "skills", &mut items);
    items.sort_by_key(|item| item.item.title.to_lowercase());
    items
}

fn scan_kind(
    layer: &LibraryLayer,
    item_type: LibraryItemType,
    dir_name: &str,
    out: &mut Vec<ParsedItem>,
) {
    let dir = layer.root.join(dir_name);
    let mut files = Vec::new();
    collect_markdown_files(&dir, &mut files);
    files.sort();
    for file in files {
        if let Some(parsed) = parse_file(layer, item_type, &file) {
            out.push(parsed);
        }
    }
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if std::fs::symlink_metadata(dir)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !matches!(name, ".git" | "node_modules" | "target" | "dist" | ".svelte-kit") {
                collect_markdown_files(&path, out);
            }
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "md" | "markdown"))
        {
            out.push(path);
        }
    }
}

fn parse_file(
    layer: &LibraryLayer,
    fallback_type: LibraryItemType,
    path: &Path,
) -> Option<ParsedItem> {
    let content = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&content)?;
    let fm: Frontmatter = serde_yaml::from_str(frontmatter).ok()?;
    let id = fm.id?.trim().to_string();
    if id.is_empty() {
        return None;
    }
    let item_type = fm.item_type.unwrap_or(fallback_type);
    if item_type != fallback_type {
        return None;
    }
    let title = fm.title.unwrap_or_else(|| title_from_path(path));
    let body = body.trim_start_matches('\n').to_string();
    let mut variables = normalize_variables(fm.variables.unwrap_or_default());
    if item_type == LibraryItemType::Prompt {
        variables = infer_body_variables(&body, variables);
    }
    Some(ParsedItem {
        item: LibraryItem {
            id,
            item_type,
            title,
            description: fm.description,
            tags: fm.tags.unwrap_or_default(),
            provider: fm.provider,
            source_layer: layer.kind,
            source_id: layer.source_id.clone(),
            source_label: layer.label.clone(),
            source_path: path.to_string_lossy().into_owned(),
            overridden_paths: Vec::new(),
            variables,
        },
        body,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Frontmatter {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
    provider: Option<String>,
    #[serde(rename = "type")]
    item_type: Option<LibraryItemType>,
    variables: Option<Vec<LibraryVariableInput>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LibraryVariableInput {
    Name(String),
    Full {
        name: String,
        label: Option<String>,
        default: Option<String>,
        required: Option<bool>,
        #[serde(rename = "type", default)]
        value_type: LibraryVariableType,
        #[serde(default)]
        options: Vec<String>,
    },
}

fn normalize_variables(inputs: Vec<LibraryVariableInput>) -> Vec<LibraryVariable> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for input in inputs {
        let variable = match input {
            LibraryVariableInput::Name(name) => LibraryVariable {
                name,
                label: None,
                default: None,
                required: true,
                value_type: LibraryVariableType::String,
                options: Vec::new(),
            },
            LibraryVariableInput::Full { name, label, default, required, value_type, options } => {
                LibraryVariable {
                    name,
                    label,
                    default,
                    required: required.unwrap_or(true),
                    value_type,
                    options: clean_options(options),
                }
            }
        };
        let name = variable.name.trim();
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        out.push(LibraryVariable { name: name.to_string(), ..variable });
    }
    out
}

fn clean_options(options: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for option in options {
        let option = option.trim().to_string();
        if !option.is_empty() && seen.insert(option.clone()) {
            out.push(option);
        }
    }
    out
}

fn infer_body_variables(body: &str, variables: Vec<LibraryVariable>) -> Vec<LibraryVariable> {
    let mut seen = variables.iter().map(|variable| variable.name.clone()).collect::<HashSet<_>>();
    let mut out = variables;
    for name in template_variable_names(body) {
        if seen.insert(name.clone()) {
            out.push(LibraryVariable {
                name,
                label: None,
                default: None,
                required: true,
                value_type: LibraryVariableType::String,
                options: Vec::new(),
            });
        }
    }
    out
}

fn validate_variable_value(variable: &LibraryVariable, value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    match variable.value_type {
        LibraryVariableType::String => Ok(()),
        LibraryVariableType::Int => {
            if is_integer_string(value) {
                Ok(())
            } else {
                Err(format!("{} must be an integer", variable.name))
            }
        }
        LibraryVariableType::Float => {
            if value.parse::<f64>().is_ok_and(|n| n.is_finite()) {
                Ok(())
            } else {
                Err(format!("{} must be a number", variable.name))
            }
        }
        LibraryVariableType::Select => {
            if variable.options.iter().any(|option| option == value) {
                Ok(())
            } else {
                Err(format!("{} must be one of the listed options", variable.name))
            }
        }
    }
}

fn is_integer_string(value: &str) -> bool {
    let rest = value.strip_prefix(['-', '+']).unwrap_or(value);
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

fn template_variable_names(input: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let mut rest = input;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            break;
        };
        let token = after_open[..close].trim();
        if is_simple_template_variable(token) && seen.insert(token.to_string()) {
            names.push(token.to_string());
        }
        rest = &after_open[close + 2..];
    }
    names
}

fn is_simple_template_variable(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\r\n").or_else(|| content.strip_prefix("---\n"))?;
    let crlf_end = rest.find("\r\n---");
    let lf_end = rest.find("\n---");
    let end = match (crlf_end, lf_end) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return None,
    };
    let (fm, after) = rest.split_at(end);
    let body = after
        .strip_prefix("\r\n---\r\n")
        .or_else(|| after.strip_prefix("\r\n---\n"))
        .or_else(|| after.strip_prefix("\r\n---"))
        .or_else(|| after.strip_prefix("\n---\r\n"))
        .or_else(|| after.strip_prefix("\n---\n"))
        .or_else(|| after.strip_prefix("\n---"))
        .unwrap_or("");
    Some((fm, body))
}

fn render_template(input: &str, variables: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let token = after_open[..close].trim();
        if is_simple_template_variable(token) {
            if let Some(value) = variables.get(token) {
                out.push_str(value);
            } else {
                out.push_str(&rest[open..open + 2 + close + 2]);
            }
        } else {
            out.push_str(&rest[open..open + 2 + close + 2]);
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

fn target_library_root(
    global_root: PathBuf,
    sources: &[LibrarySource],
    managed_sources_root: &Path,
    active_repo: Option<String>,
    target: &SaveLibraryTarget,
) -> Result<PathBuf, String> {
    match target {
        SaveLibraryTarget::Global => Ok(global_root.join("library")),
        SaveLibraryTarget::ActiveRepo => active_repo
            .filter(|repo| !repo.trim().is_empty())
            .map(|repo| Path::new(repo.trim()).join(".roux").join("library"))
            .ok_or_else(|| "no active repo library target is available".to_string()),
        SaveLibraryTarget::Source(source_id) => {
            let source = sources
                .iter()
                .find(|source| source.id == *source_id)
                .ok_or_else(|| format!("library source not found: {source_id}"))?;
            match source.kind {
                LibrarySourceKind::LocalRepo => source
                    .path
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                    .map(|path| Path::new(path.trim()).join(".roux").join("library"))
                    .ok_or_else(|| "local library source is missing a path".to_string()),
                LibrarySourceKind::GitRepo => {
                    let checkout = checkout_path_for_source(managed_sources_root, source);
                    if !crate::services::setup::git_cli().is_repo(&checkout) {
                        return Err("git library source has not been cloned yet".to_string());
                    }
                    Ok(checkout.join(".roux").join("library"))
                }
            }
        }
    }
}

fn serialize_item_markdown(
    request: &SaveLibraryItemRequest,
    item_id: &str,
    title: &str,
) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push("---".to_string());
    lines.push(format!("id: {}", yaml_string(item_id)?));
    lines.push(format!(
        "type: {}",
        match request.item_type {
            LibraryItemType::Prompt => "prompt",
            LibraryItemType::Skill => "skill",
        }
    ));
    lines.push(format!("title: {}", yaml_string(title)?));
    if let Some(description) =
        request.description.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty())
    {
        lines.push(format!("description: {}", yaml_string(description)?));
    }
    let tags =
        request.tags.iter().map(|tag| tag.trim()).filter(|tag| !tag.is_empty()).collect::<Vec<_>>();
    if !tags.is_empty() {
        let values =
            tags.iter().map(|tag| yaml_string(tag)).collect::<Result<Vec<_>, _>>()?.join(", ");
        lines.push(format!("tags: [{values}]"));
    }
    if let Some(provider) = request.provider.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        lines.push(format!("provider: {}", yaml_string(provider)?));
    }
    let variables = request
        .variables
        .iter()
        .filter(|variable| !variable.name.trim().is_empty())
        .collect::<Vec<_>>();
    if !variables.is_empty() {
        lines.push("variables:".to_string());
        for variable in variables {
            lines.push(format!("  - name: {}", yaml_string(variable.name.trim())?));
            if variable.value_type != LibraryVariableType::String {
                lines.push(format!(
                    "    type: {}",
                    match variable.value_type {
                        LibraryVariableType::String => "string",
                        LibraryVariableType::Int => "int",
                        LibraryVariableType::Float => "float",
                        LibraryVariableType::Select => "select",
                    }
                ));
            }
            if let Some(label) = variable.label.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty())
            {
                lines.push(format!("    label: {}", yaml_string(label)?));
            }
            if let Some(default) =
                variable.default.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty())
            {
                lines.push(format!("    default: {}", yaml_string(default)?));
            }
            if !variable.required {
                lines.push("    required: false".to_string());
            }
            let options = clean_options(variable.options.clone());
            if !options.is_empty() {
                let values = options
                    .iter()
                    .map(|option| yaml_string(option))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                lines.push(format!("    options: [{values}]"));
            }
        }
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(request.body.trim_start_matches('\n').to_string());
    Ok(lines.join("\n"))
}

fn yaml_string(value: &str) -> Result<String, String> {
    serde_yaml::to_string(value)
        .map_err(|e| format!("failed to serialize library metadata: {e}"))
        .map(|s| s.trim().trim_start_matches("---").trim().to_string())
}

fn is_valid_item_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn file_stem_for_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' })
        .collect()
}

fn title_from_path(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").replace(['-', '_'], " ")
}

fn safe_source_dir(id: &str) -> String {
    let sanitized: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' })
        .collect();
    if sanitized.is_empty() {
        "source".to_string()
    } else {
        sanitized
    }
}

fn remote_state_from_git(state: RemoteState) -> LibraryRemoteState {
    match state {
        RemoteState::UpToDate => LibraryRemoteState::UpToDate,
        RemoteState::Ahead => LibraryRemoteState::Ahead,
        RemoteState::Behind => LibraryRemoteState::Behind,
        RemoteState::Diverged => LibraryRemoteState::Diverged,
        RemoteState::Unknown => LibraryRemoteState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn lists_prompts_and_skills_from_layer() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        write(
            &root.join("prompts/review.md"),
            "---\nid: review.diff\ntype: prompt\ntitle: Review Diff\ntags: [review]\nvariables:\n  - goal\n---\nReview {{ goal }}\n",
        );
        write(
            &root.join("skills/rust.md"),
            "---\nid: rust.errors\ntype: skill\ntitle: Rust Errors\n---\nPrefer typed errors.\n",
        );

        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };
        let items = list_items(&[layer]);

        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .any(|item| item.id == "review.diff" && item.item_type == LibraryItemType::Prompt));
        assert!(items
            .iter()
            .any(|item| item.id == "rust.errors" && item.item_type == LibraryItemType::Skill));
    }

    #[test]
    fn parses_frontmatter_with_crlf_line_endings() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        write(
            &root.join("prompts/windows.md"),
            "---\r\nid: windows.line-endings\r\ntype: prompt\r\ntitle: Windows Line Endings\r\n---\r\nBody\r\n",
        );

        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };

        let items = list_items(&[layer]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "windows.line-endings");
    }

    #[test]
    fn higher_layers_override_lower_layers() {
        let tmp = tempfile::tempdir().unwrap();
        let global = tmp.path().join("global");
        let active = tmp.path().join("repo/.roux/library");
        write(
            &global.join("prompts/x.md"),
            "---\nid: shared\ntype: prompt\ntitle: Global\n---\nold\n",
        );
        write(
            &active.join("prompts/x.md"),
            "---\nid: shared\ntype: prompt\ntitle: Active\n---\nnew\n",
        );

        let layers = vec![
            LibraryLayer {
                kind: LibraryLayerKind::Global,
                source_id: None,
                label: "Global".into(),
                root: global,
            },
            LibraryLayer {
                kind: LibraryLayerKind::ActiveRepo,
                source_id: None,
                label: "Active repo".into(),
                root: active,
            },
        ];
        let items = list_items(&layers);
        let item = items.iter().find(|item| item.id == "shared").unwrap();

        assert_eq!(item.title, "Active");
        assert_eq!(item.source_layer, LibraryLayerKind::ActiveRepo);
        assert_eq!(item.overridden_paths.len(), 1);
    }

    #[test]
    fn renders_prompt_with_required_and_default_variables() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        write(
            &root.join("prompts/x.md"),
            "---\nid: greet\ntype: prompt\ntitle: Greet\nvariables:\n  - name: name\n  - name: punctuation\n    default: '!'\n---\nHi {{ name }}{{punctuation}}\n",
        );
        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };

        let rendered = render_prompt(
            &[layer],
            RenderLibraryPromptRequest {
                item_id: "greet".into(),
                variables: HashMap::from([("name".into(), "Sam".into())]),
                session_id: None,
            },
        )
        .unwrap();

        assert_eq!(rendered.content, "Hi Sam!\n");
    }

    #[test]
    fn infers_prompt_variables_from_body_placeholders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        write(
            &root.join("prompts/x.md"),
            "---\nid: x\ntype: prompt\ntitle: X\n---\nTest {{ blah }} and {{blah}}.\n",
        );
        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };

        let read = read_item(std::slice::from_ref(&layer), "x").unwrap();

        assert_eq!(read.item.variables.len(), 1);
        assert_eq!(read.item.variables[0].name, "blah");
        let rendered = render_prompt(
            &[layer],
            RenderLibraryPromptRequest {
                item_id: "x".into(),
                variables: HashMap::from([("blah".into(), "VALUE".into())]),
                session_id: None,
            },
        )
        .unwrap();

        assert_eq!(rendered.content, "Test VALUE and VALUE.\n");
    }

    #[test]
    fn missing_required_variable_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        write(
            &root.join("prompts/x.md"),
            "---\nid: x\ntype: prompt\ntitle: X\nvariables: [goal]\n---\n{{ goal }}\n",
        );
        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };

        let err = render_prompt(
            &[layer],
            RenderLibraryPromptRequest {
                item_id: "x".into(),
                variables: HashMap::new(),
                session_id: None,
            },
        )
        .unwrap_err();

        assert_eq!(err, "missing required variable: goal");
    }

    #[test]
    fn parses_and_validates_typed_variables() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        write(
            &root.join("prompts/x.md"),
            "---\nid: x\ntype: prompt\ntitle: X\nvariables:\n  - name: count\n    type: int\n  - name: tone\n    type: select\n    options: [friendly, direct]\n---\n{{ count }} {{ tone }}\n",
        );
        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };

        let read = read_item(std::slice::from_ref(&layer), "x").unwrap();
        assert_eq!(read.item.variables[0].value_type, LibraryVariableType::Int);
        assert_eq!(read.item.variables[1].value_type, LibraryVariableType::Select);
        assert_eq!(read.item.variables[1].options, vec!["friendly", "direct"]);

        let err = render_prompt(
            std::slice::from_ref(&layer),
            RenderLibraryPromptRequest {
                item_id: "x".into(),
                variables: HashMap::from([
                    ("count".into(), "1.5".into()),
                    ("tone".into(), "friendly".into()),
                ]),
                session_id: None,
            },
        )
        .unwrap_err();
        assert_eq!(err, "count must be an integer");

        let err = render_prompt(
            std::slice::from_ref(&layer),
            RenderLibraryPromptRequest {
                item_id: "x".into(),
                variables: HashMap::from([
                    ("count".into(), "2".into()),
                    ("tone".into(), "formal".into()),
                ]),
                session_id: None,
            },
        )
        .unwrap_err();
        assert_eq!(err, "tone must be one of the listed options");

        let rendered = render_prompt(
            &[layer],
            RenderLibraryPromptRequest {
                item_id: "x".into(),
                variables: HashMap::from([
                    ("count".into(), "2".into()),
                    ("tone".into(), "direct".into()),
                ]),
                session_id: None,
            },
        )
        .unwrap();
        assert_eq!(rendered.content, "2 direct\n");
    }

    #[test]
    fn saves_prompt_markdown_to_global_library() {
        let tmp = tempfile::tempdir().unwrap();
        let saved = save_item(
            tmp.path().to_path_buf(),
            &[],
            &tmp.path().join("managed"),
            None,
            SaveLibraryItemRequest {
                original_id: None,
                item_id: "team.review".into(),
                item_type: LibraryItemType::Prompt,
                title: "Team Review".into(),
                description: Some("Review with team defaults".into()),
                tags: vec!["review".into(), "team".into()],
                provider: None,
                variables: vec![LibraryVariable {
                    name: "focus".into(),
                    label: None,
                    default: Some("correctness".into()),
                    required: true,
                    value_type: LibraryVariableType::String,
                    options: Vec::new(),
                }],
                body: "Review {{ focus }}\n".into(),
                target: SaveLibraryTarget::Global,
                expected_source_path: None,
            },
        )
        .unwrap();

        assert!(saved.source_path.ends_with("library/prompts/team-review.md"));
        let content = std::fs::read_to_string(saved.source_path).unwrap();
        assert!(content.contains("id: team.review"));
        assert!(content.contains("variables:"));
        assert!(content.contains("Review {{ focus }}"));
    }

    #[test]
    fn save_refuses_duplicate_create() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = tmp.path().join("library/prompts/team-review.md");
        write(&existing, "---\nid: team.review\ntype: prompt\ntitle: Existing\n---\nold\n");

        let err = save_item(
            tmp.path().to_path_buf(),
            &[],
            &tmp.path().join("managed"),
            None,
            SaveLibraryItemRequest {
                original_id: None,
                item_id: "team.review".into(),
                item_type: LibraryItemType::Prompt,
                title: "Team Review".into(),
                description: None,
                tags: vec![],
                provider: None,
                variables: vec![],
                body: "new\n".into(),
                target: SaveLibraryTarget::Global,
                expected_source_path: None,
            },
        )
        .unwrap_err();

        assert!(err.contains("target file already exists"));
    }

    #[test]
    fn save_does_not_delete_client_supplied_expected_path() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = tmp.path().join("library/prompts/team-review.md");
        let victim = tmp.path().join("victim.md");
        write(&existing, "---\nid: team.review\ntype: prompt\ntitle: Existing\n---\nold\n");
        write(&victim, "do not delete me\n");

        let err = save_item(
            tmp.path().to_path_buf(),
            &[],
            &tmp.path().join("managed"),
            None,
            SaveLibraryItemRequest {
                original_id: Some("team.review".into()),
                item_id: "team.renamed".into(),
                item_type: LibraryItemType::Prompt,
                title: "Team Renamed".into(),
                description: None,
                tags: vec![],
                provider: None,
                variables: vec![],
                body: "new\n".into(),
                target: SaveLibraryTarget::Global,
                expected_source_path: Some(victim.to_string_lossy().into_owned()),
            },
        )
        .unwrap_err();

        assert_eq!(err, "source file changed on disk; reload before saving");
        assert!(victim.exists());
        assert!(existing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn scanner_skips_symlinked_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        let outside = tmp.path().join("outside");
        write(&outside.join("prompts/evil.md"), "---\nid: evil\ntype: prompt\ntitle: Evil\n---\n");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("prompts")).unwrap();

        let layer = LibraryLayer {
            kind: LibraryLayerKind::Global,
            source_id: None,
            label: "Global".into(),
            root,
        };

        assert!(list_items(&[layer]).is_empty());
    }
}
