//! Roux hooks v1 — shared event types and KDL parser.
//!
//! The initial goal is intentionally narrow:
//!
//! - parse a small `hooks.kdl` file format into plain Rust types
//! - keep routing declarative and cheap
//! - leave fine-grained applicability to the spawned hook script
//!
//! This module is the only place in `roux-core` that touches `kdl`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookConfig {
    pub hooks: Vec<HookDefinition>,
}

impl Default for HookConfig {
    fn default() -> Self {
        Self { hooks: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookDefinition {
    pub id: String,
    pub on: Vec<HookEventKind>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub filters: Vec<HookFilter>,
    pub run: HookRun,
    #[serde(default)]
    pub policy: HookPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookRun {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookPolicy {
    #[serde(default)]
    pub concurrency: Option<HookConcurrency>,
    #[serde(default)]
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HookConcurrency {
    Drop,
    Queue,
    Replace,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HookFilter {
    Equals { field: String, value: String },
    In { field: String, values: Vec<String> },
    Exists { field: String },
}

impl Default for HookDefinition {
    fn default() -> Self {
        Self {
            id: String::new(),
            on: Vec::new(),
            enabled: true,
            filters: Vec::new(),
            run: HookRun::default(),
            policy: HookPolicy::default(),
        }
    }
}

impl Default for HookRun {
    fn default() -> Self {
        Self { command: String::new(), args: Vec::new(), cwd: None, timeout_ms: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxEvent {
    pub id: String,
    pub kind: HookEventKind,
    pub timestamp: String,
    pub origin: RouxEventOrigin,
    #[serde(default)]
    pub session: Option<RouxEventSession>,
    #[serde(default)]
    pub workspace: Option<RouxEventWorkspace>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxEventOrigin {
    pub kind: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    #[serde(default)]
    pub triggered_by_hook_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxEventSession {
    #[serde(default)]
    pub roux_session_id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxEventWorkspace {
    #[serde(default)]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum HookEventKind {
    #[serde(rename = "watch.outcome_changed")]
    WatchOutcomeChanged,
    #[serde(rename = "watch.completed")]
    WatchCompleted,
    #[serde(rename = "agent.attention.entered")]
    AgentAttentionEntered,
    #[serde(rename = "agent.attention.exited")]
    AgentAttentionExited,
    #[serde(rename = "session.created")]
    SessionCreated,
    #[serde(rename = "session.ended")]
    SessionEnded,
}

impl HookEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WatchOutcomeChanged => "watch.outcome_changed",
            Self::WatchCompleted => "watch.completed",
            Self::AgentAttentionEntered => "agent.attention.entered",
            Self::AgentAttentionExited => "agent.attention.exited",
            Self::SessionCreated => "session.created",
            Self::SessionEnded => "session.ended",
        }
    }

    fn parse(value: &str, loc: (usize, usize)) -> Result<Self, HookParseError> {
        match value {
            "watch.outcome_changed" => Ok(Self::WatchOutcomeChanged),
            "watch.completed" => Ok(Self::WatchCompleted),
            "agent.attention.entered" => Ok(Self::AgentAttentionEntered),
            "agent.attention.exited" => Ok(Self::AgentAttentionExited),
            "session.created" => Ok(Self::SessionCreated),
            "session.ended" => Ok(Self::SessionEnded),
            other => Err(HookParseError::schema(
                loc,
                format!(
                    "unknown hook event `{other}`; expected one of `watch.outcome_changed`, `watch.completed`, `agent.attention.entered`, `agent.attention.exited`, `session.created`, or `session.ended`"
                ),
            )),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum HookParseError {
    #[error("kdl parse error at {line}:{column}: {message}")]
    Syntax { line: usize, column: usize, message: String },
    #[error("{message} at {line}:{column}")]
    Schema { line: usize, column: usize, message: String },
}

impl HookParseError {
    fn schema(loc: (usize, usize), message: impl Into<String>) -> Self {
        Self::Schema { line: loc.0, column: loc.1, message: message.into() }
    }
}

pub fn parse_hooks_kdl(src: &str) -> Result<HookConfig, HookParseError> {
    let doc: kdl::KdlDocument = src.parse().map_err(|e: kdl::KdlError| {
        let (line, column, message) = e
            .diagnostics
            .first()
            .map(|d| {
                let (l, c) = offset_to_line_col(src, d.span.offset());
                (l, c, d.message.clone().unwrap_or_else(|| "invalid KDL syntax".to_string()))
            })
            .unwrap_or_else(|| (0, 0, "invalid KDL syntax".to_string()));
        HookParseError::Syntax { line, column, message }
    })?;

    let top_nodes: Vec<&kdl::KdlNode> = doc.nodes().iter().collect();
    let hooks_node = match top_nodes.as_slice() {
        [single] if single.name().value() == "hooks" => *single,
        [] => {
            return Err(HookParseError::schema(
                (1, 1),
                "expected a top-level `hooks` node; document is empty",
            ));
        }
        [single] => {
            return Err(HookParseError::schema(
                node_loc(src, single),
                format!(
                    "unknown top-level node `{}`; expected a top-level `hooks` node",
                    single.name().value()
                ),
            ));
        }
        [_, second, ..] => {
            return Err(HookParseError::schema(
                node_loc(src, second),
                "exactly one top-level `hooks` node is permitted",
            ));
        }
    };

    parse_hooks_node(src, hooks_node)
}

pub fn merge_hook_configs(base: &HookConfig, overlay: &HookConfig) -> HookConfig {
    let mut hooks = base.hooks.clone();
    for overlay_hook in &overlay.hooks {
        if let Some(idx) = hooks.iter().position(|existing| existing.id == overlay_hook.id) {
            hooks[idx] = overlay_hook.clone();
        } else {
            hooks.push(overlay_hook.clone());
        }
    }
    hooks.retain(|hook| hook.enabled);
    HookConfig { hooks }
}

fn parse_hooks_node(src: &str, node: &kdl::KdlNode) -> Result<HookConfig, HookParseError> {
    if let Some(entry) = node.entries().iter().next() {
        return Err(HookParseError::schema(
            entry_loc(src, entry),
            "`hooks` node takes no arguments; define child `hook` nodes inside it",
        ));
    }
    let body = node
        .children()
        .ok_or_else(|| HookParseError::schema(node_loc(src, node), "`hooks` must have a body"))?;

    let mut hooks = Vec::new();
    for child in body.nodes() {
        match child.name().value() {
            "hook" => hooks.push(parse_hook_node(src, child)?),
            other => {
                return Err(HookParseError::schema(
                    node_loc(src, child),
                    format!("unknown child node `{other}` in `hooks`; expected `hook`"),
                ));
            }
        }
    }
    Ok(HookConfig { hooks })
}

fn parse_hook_node(src: &str, node: &kdl::KdlNode) -> Result<HookDefinition, HookParseError> {
    let id = single_string_arg(src, node, "hook")?;
    let body = node.children().ok_or_else(|| {
        HookParseError::schema(node_loc(src, node), "`hook` must have a body `{ ... }`")
    })?;

    let mut hook = HookDefinition { id, ..HookDefinition::default() };

    for child in body.nodes() {
        match child.name().value() {
            "on" => {
                let raw = single_string_arg(src, child, "on")?;
                hook.on.push(HookEventKind::parse(&raw, node_loc(src, child))?);
            }
            "enabled" => hook.enabled = single_bool_arg(src, child, "enabled")?,
            "run" => {
                if hook.run.command.is_empty() {
                    hook.run = parse_run_node(src, child)?;
                } else {
                    return Err(HookParseError::schema(
                        node_loc(src, child),
                        "duplicate `run` block",
                    ));
                }
            }
            "policy" => {
                if hook.policy == HookPolicy::default() {
                    hook.policy = parse_policy_node(src, child)?;
                } else {
                    return Err(HookParseError::schema(
                        node_loc(src, child),
                        "duplicate `policy` block",
                    ));
                }
            }
            "filter" => hook.filters.push(parse_filter_node(src, child)?),
            other => {
                return Err(HookParseError::schema(
                    node_loc(src, child),
                    format!(
                        "unknown child node `{other}` in `hook`; expected `on`, `enabled`, `run`, `policy`, or `filter`"
                    ),
                ));
            }
        }
    }

    if hook.on.is_empty() {
        return Err(HookParseError::schema(
            node_loc(src, node),
            "`hook` must contain at least one `on` child node",
        ));
    }
    if hook.run.command.is_empty() {
        return Err(HookParseError::schema(
            node_loc(src, node),
            "`hook` is missing required `run` block",
        ));
    }

    Ok(hook)
}

fn parse_run_node(src: &str, node: &kdl::KdlNode) -> Result<HookRun, HookParseError> {
    if let Some(entry) = node.entries().iter().next() {
        return Err(HookParseError::schema(
            entry_loc(src, entry),
            "`run` node takes no arguments; configure child nodes inside it",
        ));
    }
    let body = node
        .children()
        .ok_or_else(|| HookParseError::schema(node_loc(src, node), "`run` must have a body"))?;

    let mut run = HookRun::default();
    for child in body.nodes() {
        match child.name().value() {
            "command" => {
                if run.command.is_empty() {
                    run.command = single_string_arg(src, child, "command")?;
                } else {
                    return Err(HookParseError::schema(
                        node_loc(src, child),
                        "duplicate `command` in `run` block",
                    ));
                }
            }
            "arg" => run.args.push(single_string_arg(src, child, "arg")?),
            "cwd" => run.cwd = Some(single_string_arg(src, child, "cwd")?),
            "timeout-ms" => run.timeout_ms = Some(single_u64_arg(src, child, "timeout-ms")?),
            other => {
                return Err(HookParseError::schema(
                    node_loc(src, child),
                    format!(
                        "unknown child node `{other}` in `run`; expected `command`, `arg`, `cwd`, or `timeout-ms`"
                    ),
                ));
            }
        }
    }

    if run.command.is_empty() {
        return Err(HookParseError::schema(
            node_loc(src, node),
            "`run` is missing required `command` child node",
        ));
    }
    Ok(run)
}

fn parse_policy_node(src: &str, node: &kdl::KdlNode) -> Result<HookPolicy, HookParseError> {
    if let Some(entry) = node.entries().iter().next() {
        return Err(HookParseError::schema(
            entry_loc(src, entry),
            "`policy` node takes no arguments; configure child nodes inside it",
        ));
    }
    let body = node.children().ok_or_else(|| {
        HookParseError::schema(node_loc(src, node), "`policy` must have a body")
    })?;

    let mut policy = HookPolicy::default();
    for child in body.nodes() {
        match child.name().value() {
            "concurrency" => {
                policy.concurrency = Some(parse_concurrency(src, child)?);
            }
            "dedupe-key" => {
                policy.dedupe_key = Some(single_string_arg(src, child, "dedupe-key")?);
            }
            other => {
                return Err(HookParseError::schema(
                    node_loc(src, child),
                    format!(
                        "unknown child node `{other}` in `policy`; expected `concurrency` or `dedupe-key`"
                    ),
                ));
            }
        }
    }
    Ok(policy)
}

fn parse_filter_node(src: &str, node: &kdl::KdlNode) -> Result<HookFilter, HookParseError> {
    let field = string_prop(src, node, "field")?;
    if let Some(value) = optional_string_prop(src, node, "equals")? {
        return Ok(HookFilter::Equals { field, value });
    }
    if let Some(value) = optional_string_prop(src, node, "in")? {
        let values = value
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        return Ok(HookFilter::In { field, values });
    }
    if optional_bool_prop(src, node, "exists")?.unwrap_or(false) {
        return Ok(HookFilter::Exists { field });
    }
    Err(HookParseError::schema(
        node_loc(src, node),
        "`filter` requires one of `equals`, `in`, or `exists=true`",
    ))
}

fn parse_concurrency(src: &str, node: &kdl::KdlNode) -> Result<HookConcurrency, HookParseError> {
    match single_string_arg(src, node, "concurrency")?.as_str() {
        "drop" => Ok(HookConcurrency::Drop),
        "queue" => Ok(HookConcurrency::Queue),
        "replace" => Ok(HookConcurrency::Replace),
        "parallel" => Ok(HookConcurrency::Parallel),
        other => Err(HookParseError::schema(
            node_loc(src, node),
            format!(
                "unknown concurrency `{other}`; expected `drop`, `queue`, `replace`, or `parallel`"
            ),
        )),
    }
}

fn single_string_arg(
    src: &str,
    node: &kdl::KdlNode,
    what: &str,
) -> Result<String, HookParseError> {
    let entries: Vec<&kdl::KdlEntry> = node.entries().iter().collect();
    match entries.as_slice() {
        [entry] => match entry.value() {
            kdl::KdlValue::String(value) => Ok(value.clone()),
            _ => Err(HookParseError::schema(
                entry_loc(src, entry),
                format!("`{what}` requires a string argument"),
            )),
        },
        [] => Err(HookParseError::schema(
            node_loc(src, node),
            format!("`{what}` requires one string argument"),
        )),
        [_, second, ..] => Err(HookParseError::schema(
            entry_loc(src, second),
            format!("`{what}` accepts exactly one argument"),
        )),
    }
}

fn single_u64_arg(src: &str, node: &kdl::KdlNode, what: &str) -> Result<u64, HookParseError> {
    let entries: Vec<&kdl::KdlEntry> = node.entries().iter().collect();
    match entries.as_slice() {
        [entry] => match entry.value() {
            kdl::KdlValue::Integer(value) if *value >= 0 => Ok(*value as u64),
            _ => Err(HookParseError::schema(
                entry_loc(src, entry),
                format!("`{what}` requires a non-negative integer argument"),
            )),
        },
        [] => Err(HookParseError::schema(
            node_loc(src, node),
            format!("`{what}` requires one integer argument"),
        )),
        [_, second, ..] => Err(HookParseError::schema(
            entry_loc(src, second),
            format!("`{what}` accepts exactly one argument"),
        )),
    }
}

fn single_bool_arg(src: &str, node: &kdl::KdlNode, what: &str) -> Result<bool, HookParseError> {
    let entries: Vec<&kdl::KdlEntry> = node.entries().iter().collect();
    match entries.as_slice() {
        [entry] => match entry.value() {
            kdl::KdlValue::Bool(value) => Ok(*value),
            _ => Err(HookParseError::schema(
                entry_loc(src, entry),
                format!("`{what}` requires a boolean argument"),
            )),
        },
        [] => Err(HookParseError::schema(
            node_loc(src, node),
            format!("`{what}` requires one boolean argument"),
        )),
        [_, second, ..] => Err(HookParseError::schema(
            entry_loc(src, second),
            format!("`{what}` accepts exactly one argument"),
        )),
    }
}

fn string_prop(src: &str, node: &kdl::KdlNode, key: &str) -> Result<String, HookParseError> {
    optional_string_prop(src, node, key)?.ok_or_else(|| {
        HookParseError::schema(node_loc(src, node), format!("`filter` requires `{key}` property"))
    })
}

fn optional_string_prop(
    src: &str,
    node: &kdl::KdlNode,
    key: &str,
) -> Result<Option<String>, HookParseError> {
    let Some(value) = node.get(key) else {
        return Ok(None);
    };
    match value {
        kdl::KdlValue::String(value) => Ok(Some(value.clone())),
        _ => Err(HookParseError::schema(
            node_loc(src, node),
            format!("property `{key}` requires a string value"),
        )),
    }
}

fn optional_bool_prop(
    src: &str,
    node: &kdl::KdlNode,
    key: &str,
) -> Result<Option<bool>, HookParseError> {
    let Some(value) = node.get(key) else {
        return Ok(None);
    };
    match value {
        kdl::KdlValue::Bool(value) => Ok(Some(*value)),
        _ => Err(HookParseError::schema(
            node_loc(src, node),
            format!("property `{key}` requires a boolean value"),
        )),
    }
}

fn offset_to_line_col(src: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let prefix = &src[..offset];
    let line = 1 + prefix.matches('\n').count();
    let column = match prefix.rfind('\n') {
        Some(nl) => prefix[nl + 1..].chars().count() + 1,
        None => prefix.chars().count() + 1,
    };
    (line, column)
}

fn node_loc(src: &str, node: &kdl::KdlNode) -> (usize, usize) {
    offset_to_line_col(src, node.span().offset())
}

fn entry_loc(src: &str, entry: &kdl::KdlEntry) -> (usize, usize) {
    offset_to_line_col(src, entry.span().offset())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_hook_config() {
        let src = r#"
            hooks {
              hook "pr-watch-auto-fix" {
                on "watch.completed"

                run {
                  command "python3"
                  arg ".roux/scripts/pr_watch.py"
                  cwd "worktree"
                  timeout-ms 300000
                }

                policy {
                  concurrency "replace"
                }
              }
            }
        "#;

        let parsed = parse_hooks_kdl(src).expect("parse ok");
        assert_eq!(parsed.hooks.len(), 1);

        let hook = &parsed.hooks[0];
        assert_eq!(hook.id, "pr-watch-auto-fix");
        assert_eq!(hook.on, vec![HookEventKind::WatchCompleted]);
        assert_eq!(hook.run.command, "python3");
        assert_eq!(hook.run.args, vec![".roux/scripts/pr_watch.py"]);
        assert_eq!(hook.run.cwd.as_deref(), Some("worktree"));
        assert_eq!(hook.run.timeout_ms, Some(300000));
        assert_eq!(hook.policy.concurrency, Some(HookConcurrency::Replace));
        assert!(hook.enabled, "hooks default to enabled");
    }

    #[test]
    fn rejects_unknown_hook_event_kind() {
        let err = parse_hooks_kdl(
            r#"
                hooks {
                  hook "bad" {
                    on "watch.failed_hard"
                    run { command "python3" }
                  }
                }
            "#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown hook event") && msg.contains("watch.failed_hard"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn rejects_unknown_top_level_node() {
        let err = parse_hooks_kdl("wat garbage").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("top-level `hooks` node") || msg.contains("invalid KDL syntax"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn merge_replaces_same_id_and_disabled_overlay_suppresses_base() {
        let base = parse_hooks_kdl(
            r#"
                hooks {
                  hook "keep-me" {
                    on "watch.completed"
                    run { command "python3" }
                  }
                  hook "replace-me" {
                    on "watch.completed"
                    run {
                      command "python3"
                      arg "base.py"
                    }
                  }
                  hook "suppress-me" {
                    on "watch.completed"
                    run {
                      command "python3"
                      arg "base.py"
                    }
                  }
                }
            "#,
        )
        .expect("base parse ok");
        let overlay = parse_hooks_kdl(
            r#"
                hooks {
                  hook "replace-me" {
                    on "watch.completed"
                    run {
                      command "python3"
                      arg "overlay.py"
                    }
                  }
                  hook "suppress-me" {
                    enabled #false
                    on "watch.completed"
                    run {
                      command "python3"
                      arg "overlay.py"
                    }
                  }
                }
            "#,
        )
        .expect("overlay parse ok");

        let merged = merge_hook_configs(&base, &overlay);
        assert_eq!(merged.hooks.len(), 2);
        assert_eq!(merged.hooks[0].id, "keep-me");
        assert_eq!(merged.hooks[1].id, "replace-me");
        assert_eq!(merged.hooks[1].run.args, vec!["overlay.py"]);
    }
}
