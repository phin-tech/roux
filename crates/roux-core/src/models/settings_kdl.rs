//! KDL serialization for [`super::settings::RouxSettings`].
//!
//! This module is the ONLY place in the codebase that knows about `kdl-rs`
//! for settings. Callers above interact with plain `&str` / `String` and the
//! `RouxSettings` struct. If you need to plumb more KDL surface into the
//! settings schema, add it here and keep the leakage boundary intact —
//! mirroring the encapsulation rule in `layout.rs`.
//!
//! ## Why this module exists
//!
//! Settings used to be stored as `settings.json` and round-tripped through
//! `serde_json`. Because the in-app Settings panel rewrites the file every
//! time a value changes, a struct-only round trip would destroy any user
//! comments or whitespace on every keystroke. KDL's CST representation lets
//! us mutate a parsed document *in place*, preserving every node, comment,
//! and blank line we did not explicitly touch.
//!
//! ## Public surface
//!
//! - [`parse`] — KDL text → [`RouxSettings`] (does not run [`RouxSettings::normalized`]).
//! - [`render_default`] — produce a brand-new file with section comments and
//!   default values. Used both for the initial settings file when no prior
//!   file exists and as the seed document for [`apply`] in that case.
//! - [`apply`] — splice values from a [`RouxSettings`] into existing KDL
//!   text, preserving comments and unrelated nodes.
//!
//! ## Comment-preservation trade-offs
//!
//! For scalar fields we update the existing node's value in place, so any
//! surrounding comment survives. For list-shaped fields (`repo_root`,
//! `additional_flag`, `trusted_workspace`, `spawn_profile`) we remove every
//! node with the relevant name and re-emit from the struct. This preserves
//! comments *around* the list block but not interleaved between individual
//! list entries — acceptable because those entries are user-managed via the
//! UI for the common case.
//!
//! The legacy `cleanup_worktrees_on_close` boolean is accepted on parse for
//! back-compat (so a hand-edit does not get silently dropped) but is never
//! emitted on write. `RouxSettings::normalized` already promotes it to the
//! `worktree_cleanup_on_close` enum.

use std::str::FromStr;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};

use super::profile::{ProfileSource, Provider, SpawnProfile, StartupBehavior};
use super::settings::RouxSettings;

// ---------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------

/// Errors from [`parse`] and [`apply`].
///
/// Carries 1-indexed line/column for diagnostics. Not a `specta::Type` —
/// errors cross the IPC boundary as `String`, matching every other service
/// error in roux.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SettingsKdlError {
    #[error("kdl parse error at {line}:{column}: {message}")]
    Syntax { line: usize, column: usize, message: String },
    #[error("{message} at {line}:{column}")]
    Schema { line: usize, column: usize, message: String },
}

impl SettingsKdlError {
    fn schema(loc: (usize, usize), message: impl Into<String>) -> Self {
        Self::Schema { line: loc.0, column: loc.1, message: message.into() }
    }
}

/// Parse a settings KDL document into a [`RouxSettings`].
///
/// Missing fields fall back to [`RouxSettings::default`]. Unknown nodes are
/// ignored (intentional: protects users who downgrade after we add a field,
/// matching the `#[serde(default)]` philosophy on the struct).
///
/// The caller is responsible for invoking [`RouxSettings::normalized`] on
/// the result if normalization is desired (the on-disk loader does).
pub fn parse(input: &str) -> Result<RouxSettings, SettingsKdlError> {
    let doc = parse_document(input)?;
    let mut settings = RouxSettings::default();
    populate_from_document(&doc, input, &mut settings)?;
    Ok(settings)
}

/// Render a fresh settings KDL document with section headers, helpful
/// comments, and default values.
///
/// Used as the initial file content when no prior settings file exists,
/// and as the seed document for [`apply`] in that same case.
pub fn render_default() -> String {
    render_from_scratch(&RouxSettings::default())
}

/// Splice values from `settings` into the existing KDL text, returning the
/// updated document.
///
/// Comments, blank lines, and nodes we do not recognize survive untouched.
/// If `existing` is empty or whitespace-only, this behaves like
/// [`render_default`] with the values from `settings`.
///
/// Does NOT call [`RouxSettings::normalized`] on the input — the caller is
/// expected to pass an already-normalized struct (the on-disk saver does).
pub fn apply(existing: &str, settings: &RouxSettings) -> Result<String, SettingsKdlError> {
    if existing.trim().is_empty() {
        return Ok(render_from_scratch(settings));
    }
    let mut doc = parse_document(existing)?;
    apply_to_document(&mut doc, settings);
    Ok(doc.to_string())
}

// ---------------------------------------------------------------------------
// document parsing
// ---------------------------------------------------------------------------

fn parse_document(input: &str) -> Result<KdlDocument, SettingsKdlError> {
    input.parse::<KdlDocument>().map_err(|e| {
        let (line, column, message) = e
            .diagnostics
            .first()
            .map(|d| {
                let (l, c) = offset_to_line_col(input, d.span.offset());
                (l, c, d.message.clone().unwrap_or_else(|| "invalid KDL syntax".to_string()))
            })
            .unwrap_or_else(|| (0, 0, "invalid KDL syntax".to_string()));
        SettingsKdlError::Syntax { line, column, message }
    })
}

/// Walk the top-level document, dispatching each known section node to its
/// handler. Unknown sections and unknown child nodes are ignored, by design
/// (see the module docstring).
fn populate_from_document(
    doc: &KdlDocument,
    src: &str,
    out: &mut RouxSettings,
) -> Result<(), SettingsKdlError> {
    out.spawn_profiles.clear();
    out.repo_roots.clear();
    out.trusted_workspaces.clear();
    out.additional_flags.clear();

    for node in doc.nodes() {
        match node.name().value() {
            "ui" => populate_ui(node, src, out)?,
            "terminal" => populate_terminal(node, src, out)?,
            "sessions" => populate_sessions(node, src, out)?,
            "worktrees" => populate_worktrees(node, src, out)?,
            "claude" => populate_claude(node, src, out)?,
            "integrations" => populate_integrations(node, src, out)?,
            "notifications" => populate_notifications(node, src, out)?,
            "keyboard" => populate_keyboard(node, src, out)?,
            "advanced" => populate_advanced(node, src, out)?,
            "trusted_workspace" => {
                out.trusted_workspaces.push(string_arg(node, src, "trusted_workspace")?);
            }
            "spawn_profile" => {
                out.spawn_profiles.push(parse_spawn_profile(node, src)?);
            }
            _ => {
                // Unknown top-level node: ignore silently. A future field
                // may legitimately use this name.
            }
        }
    }
    Ok(())
}

fn populate_ui(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "theme" => out.theme = string_arg(child, src, "theme")?,
            "ui_font_family" => out.ui_font_family = string_arg(child, src, "ui_font_family")?,
            "tab_position" => out.tab_position = parse_enum(child, src, "tab_position")?,
            "tab_width" => out.tab_width = u32_arg(child, src, "tab_width")?,
            "sidebar_collapsed" => out.sidebar_collapsed = bool_arg(child, src, "sidebar_collapsed")?,
            "status_bar_position" => out.status_bar_position = parse_enum(child, src, "status_bar_position")?,
            "task_panel_split" => out.task_panel_split = f64_arg(child, src, "task_panel_split")?,
            "task_panel_collapsed" => out.task_panel_collapsed = bool_arg(child, src, "task_panel_collapsed")?,
            _ => {}
        }
    }
    Ok(())
}

fn populate_terminal(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "font_family" => out.font_family = string_arg(child, src, "font_family")?,
            "font_size" => out.font_size = u32_arg(child, src, "font_size")?,
            "line_height" => out.line_height = f64_arg(child, src, "line_height")?,
            "scrollback" => out.scrollback = u32_arg(child, src, "scrollback")?,
            "cursor_style" => out.cursor_style = parse_enum(child, src, "cursor_style")?,
            "cursor_blink" => out.cursor_blink = bool_arg(child, src, "cursor_blink")?,
            _ => {}
        }
    }
    Ok(())
}

fn populate_sessions(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "confirm_on_close" => out.confirm_on_close = bool_arg(child, src, "confirm_on_close")?,
            "confirm_on_quit" => out.confirm_on_quit = bool_arg(child, src, "confirm_on_quit")?,
            "restore_sessions_on_launch" => {
                out.restore_sessions_on_launch = bool_arg(child, src, "restore_sessions_on_launch")?
            }
            "group_by" => out.group_by = parse_enum(child, src, "group_by")?,
            _ => {}
        }
    }
    Ok(())
}

fn populate_worktrees(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "worktree_base_path" => out.worktree_base_path = optional_string_arg(child, src, "worktree_base_path")?,
            "worktree_cleanup_on_close" => {
                out.worktree_cleanup_on_close = parse_enum(child, src, "worktree_cleanup_on_close")?
            }
            "exclude_worktrees_from_repo_roots" => {
                out.exclude_worktrees_from_repo_roots =
                    bool_arg(child, src, "exclude_worktrees_from_repo_roots")?
            }
            // Legacy: accepted on parse so a hand-edit isn't silently lost.
            // `RouxSettings::normalized` promotes this to the enum on load.
            "cleanup_worktrees_on_close" => {
                out.cleanup_worktrees_on_close = bool_arg(child, src, "cleanup_worktrees_on_close")?
            }
            "repo_root" => out.repo_roots.push(string_arg(child, src, "repo_root")?),
            _ => {}
        }
    }
    Ok(())
}

fn populate_claude(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "default_model" => out.default_model = optional_string_arg(child, src, "default_model")?,
            "claude_binary_path" => out.claude_binary_path = optional_string_arg(child, src, "claude_binary_path")?,
            "additional_flag" => out.additional_flags.push(string_arg(child, src, "additional_flag")?),
            _ => {}
        }
    }
    Ok(())
}

fn populate_integrations(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        if child.name().value() == "gh_binary_path" {
            out.gh_binary_path = optional_string_arg(child, src, "gh_binary_path")?;
        }
    }
    Ok(())
}

fn populate_notifications(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "notifications_enabled" => out.notifications_enabled = bool_arg(child, src, "notifications_enabled")?,
            "auto_clear_attention_state" => {
                out.auto_clear_attention_state = bool_arg(child, src, "auto_clear_attention_state")?
            }
            _ => {}
        }
    }
    Ok(())
}

fn populate_keyboard(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "show_pane_hints_on_option" => {
                out.show_pane_hints_on_option = bool_arg(child, src, "show_pane_hints_on_option")?
            }
            "show_session_hints_on_command" => {
                out.show_session_hints_on_command = bool_arg(child, src, "show_session_hints_on_command")?
            }
            _ => {}
        }
    }
    Ok(())
}

fn populate_advanced(node: &KdlNode, src: &str, out: &mut RouxSettings) -> Result<(), SettingsKdlError> {
    for child in section_children(node) {
        match child.name().value() {
            "enable_logging" => out.enable_logging = bool_arg(child, src, "enable_logging")?,
            "update_check_on_launch" => out.update_check_on_launch = bool_arg(child, src, "update_check_on_launch")?,
            "default_project_path" => {
                out.default_project_path = optional_string_arg(child, src, "default_project_path")?
            }
            _ => {}
        }
    }
    Ok(())
}

/// Return the children of a section node, or an empty slice if it has no body.
fn section_children(node: &KdlNode) -> &[KdlNode] {
    match node.children() {
        Some(doc) => doc.nodes(),
        None => &[],
    }
}

// ---------------------------------------------------------------------------
// scalar parsing helpers
// ---------------------------------------------------------------------------

fn first_positional<'a>(node: &'a KdlNode) -> Option<&'a KdlEntry> {
    node.entries().iter().find(|e| e.name().is_none())
}

fn string_arg(node: &KdlNode, src: &str, what: &str) -> Result<String, SettingsKdlError> {
    let entry = first_positional(node)
        .ok_or_else(|| SettingsKdlError::schema(node_loc(src, node), format!("`{what}` requires a string argument")))?;
    match entry.value() {
        KdlValue::String(s) => Ok(s.clone()),
        _ => Err(SettingsKdlError::schema(entry_loc(src, entry), format!("`{what}` must be a string"))),
    }
}

fn optional_string_arg(node: &KdlNode, src: &str, what: &str) -> Result<Option<String>, SettingsKdlError> {
    let entry = first_positional(node).ok_or_else(|| {
        SettingsKdlError::schema(node_loc(src, node), format!("`{what}` requires an argument (string or null)"))
    })?;
    match entry.value() {
        KdlValue::String(s) => Ok(Some(s.clone())),
        KdlValue::Null => Ok(None),
        _ => Err(SettingsKdlError::schema(entry_loc(src, entry), format!("`{what}` must be a string or null"))),
    }
}

fn bool_arg(node: &KdlNode, src: &str, what: &str) -> Result<bool, SettingsKdlError> {
    let entry = first_positional(node)
        .ok_or_else(|| SettingsKdlError::schema(node_loc(src, node), format!("`{what}` requires a boolean argument")))?;
    match entry.value() {
        KdlValue::Bool(b) => Ok(*b),
        _ => Err(SettingsKdlError::schema(entry_loc(src, entry), format!("`{what}` must be true or false"))),
    }
}

fn u32_arg(node: &KdlNode, src: &str, what: &str) -> Result<u32, SettingsKdlError> {
    let n = number_arg(node, src, what)?;
    if n.fract() != 0.0 || n < 0.0 || n > u32::MAX as f64 {
        return Err(SettingsKdlError::schema(
            node_loc(src, node),
            format!("`{what}` must be a non-negative integer that fits in u32; got {n}"),
        ));
    }
    Ok(n as u32)
}

fn f64_arg(node: &KdlNode, src: &str, what: &str) -> Result<f64, SettingsKdlError> {
    number_arg(node, src, what)
}

fn number_arg(node: &KdlNode, src: &str, what: &str) -> Result<f64, SettingsKdlError> {
    let entry = first_positional(node)
        .ok_or_else(|| SettingsKdlError::schema(node_loc(src, node), format!("`{what}` requires a numeric argument")))?;
    match entry.value() {
        KdlValue::Integer(i) => Ok(*i as f64),
        KdlValue::Float(f) => Ok(*f),
        _ => Err(SettingsKdlError::schema(entry_loc(src, entry), format!("`{what}` must be a number"))),
    }
}

/// Parse a string-valued enum via serde_json (single source of truth for
/// the camelCase mapping the rest of the codebase already uses).
fn parse_enum<T: serde::de::DeserializeOwned>(
    node: &KdlNode,
    src: &str,
    what: &str,
) -> Result<T, SettingsKdlError> {
    let s = string_arg(node, src, what)?;
    serde_json::from_value::<T>(serde_json::Value::String(s.clone())).map_err(|_| {
        SettingsKdlError::schema(node_loc(src, node), format!("`{what}` has invalid value `{s}`"))
    })
}

// ---------------------------------------------------------------------------
// spawn profile parsing
// ---------------------------------------------------------------------------

/// Parse a `spawn_profile id="..." name="..." { ... }` node.
///
/// Attribute layout matches the existing JSON shape one-for-one. Optional
/// fields not present fall back to `None` / defaults. `source` is forced to
/// [`ProfileSource::User`] regardless — the on-disk loader's
/// `RouxSettings::normalized` would do this anyway, but we keep the
/// invariant here so a `parse → use` path that skips normalization (e.g. a
/// future test) can't observe a forged `Builtin`.
fn parse_spawn_profile(node: &KdlNode, src: &str) -> Result<SpawnProfile, SettingsKdlError> {
    let mut id: Option<String> = None;
    let mut name: Option<String> = None;
    let mut icon: Option<String> = None;
    let mut provider: Option<Provider> = None;
    let mut nono_profile: Option<String> = None;

    for entry in node.entries() {
        let Some(attr) = entry.name().map(|i| i.value()) else {
            return Err(SettingsKdlError::schema(
                entry_loc(src, entry),
                "`spawn_profile` does not take positional arguments; use key=value attributes",
            ));
        };
        match attr {
            "id" => id = Some(entry_string(entry, src, "id")?),
            "name" => name = Some(entry_string(entry, src, "name")?),
            "icon" => icon = Some(entry_string(entry, src, "icon")?),
            "provider" => {
                let s = entry_string(entry, src, "provider")?;
                provider = Some(serde_json::from_value(serde_json::Value::String(s.clone())).map_err(|_| {
                    SettingsKdlError::schema(entry_loc(src, entry), format!("invalid provider `{s}`"))
                })?);
            }
            "nono_profile" => nono_profile = Some(entry_string(entry, src, "nono_profile")?),
            _ => {
                // Unknown attribute: ignore. Future fields may use it.
            }
        }
    }

    let id = id.ok_or_else(|| SettingsKdlError::schema(node_loc(src, node), "`spawn_profile` requires id=\"...\""))?;
    let name = name.ok_or_else(|| SettingsKdlError::schema(node_loc(src, node), "`spawn_profile` requires name=\"...\""))?;

    let mut setup_command: Option<String> = None;
    let mut startup_command: Option<String> = None;
    let mut startup_behavior: Option<StartupBehavior> = None;
    let mut cwd_override: Option<String> = None;
    let mut env: Option<std::collections::BTreeMap<String, String>> = None;
    let mut nono_allow_dirs: Option<Vec<String>> = None;

    if let Some(body) = node.children() {
        for child in body.nodes() {
            match child.name().value() {
                "setup_command" => setup_command = Some(string_arg(child, src, "setup_command")?),
                "startup_command" => startup_command = Some(string_arg(child, src, "startup_command")?),
                "startup_behavior" => startup_behavior = Some(parse_enum(child, src, "startup_behavior")?),
                "cwd_override" => cwd_override = Some(string_arg(child, src, "cwd_override")?),
                "nono_allow_dir" => {
                    nono_allow_dirs.get_or_insert_with(Vec::new).push(string_arg(child, src, "nono_allow_dir")?)
                }
                "env" => {
                    let mut map = std::collections::BTreeMap::new();
                    if let Some(env_body) = child.children() {
                        for kv in env_body.nodes() {
                            let key = kv.name().value().to_string();
                            let value = string_arg(kv, src, &key)?;
                            map.insert(key, value);
                        }
                    }
                    env = Some(map);
                }
                _ => {}
            }
        }
    }

    Ok(SpawnProfile {
        id,
        name,
        setup_command,
        startup_command,
        startup_behavior,
        env,
        cwd_override,
        icon,
        provider,
        nono_profile,
        nono_allow_dirs,
        source: ProfileSource::User,
    })
}

fn entry_string(entry: &KdlEntry, src: &str, what: &str) -> Result<String, SettingsKdlError> {
    match entry.value() {
        KdlValue::String(s) => Ok(s.clone()),
        _ => Err(SettingsKdlError::schema(entry_loc(src, entry), format!("`{what}` must be a string"))),
    }
}

// ---------------------------------------------------------------------------
// rendering — fresh document
// ---------------------------------------------------------------------------

/// Produce a fully-populated KDL document with section comments. Used only
/// when no prior file exists; `apply` re-uses an existing document.
///
/// Uses `KdlDocument::autoformat` for pretty indentation. We do NOT
/// autoformat in the `apply` path because that would rewrite user trivia;
/// here the document has no user-authored content to preserve, so a fresh
/// pretty-print is the right call.
fn render_from_scratch(settings: &RouxSettings) -> String {
    let mut out = String::new();
    out.push_str("// Roux settings. Edit by hand or via the in-app Settings panel.\n");
    out.push_str("// Comments and blank lines are preserved when the UI rewrites this file.\n\n");

    let mut doc = KdlDocument::new();
    apply_to_document(&mut doc, settings);
    doc.autoformat();
    out.push_str(&doc.to_string());
    out
}

// ---------------------------------------------------------------------------
// apply — surgical patch
// ---------------------------------------------------------------------------

/// Apply `settings` to `doc` in place. Public to the module only — callers
/// outside `settings_kdl` go through [`apply`] which works on `&str`.
fn apply_to_document(doc: &mut KdlDocument, settings: &RouxSettings) {
    apply_section(doc, "ui", &[
        ScalarField::String("theme", &settings.theme),
        ScalarField::String("ui_font_family", &settings.ui_font_family),
        ScalarField::EnumStr("tab_position", enum_to_string(&settings.tab_position)),
        ScalarField::U32("tab_width", settings.tab_width),
        ScalarField::Bool("sidebar_collapsed", settings.sidebar_collapsed),
        ScalarField::EnumStr("status_bar_position", enum_to_string(&settings.status_bar_position)),
        ScalarField::F64("task_panel_split", settings.task_panel_split),
        ScalarField::Bool("task_panel_collapsed", settings.task_panel_collapsed),
    ]);

    apply_section(doc, "terminal", &[
        ScalarField::String("font_family", &settings.font_family),
        ScalarField::U32("font_size", settings.font_size),
        ScalarField::F64("line_height", settings.line_height),
        ScalarField::U32("scrollback", settings.scrollback),
        ScalarField::EnumStr("cursor_style", enum_to_string(&settings.cursor_style)),
        ScalarField::Bool("cursor_blink", settings.cursor_blink),
    ]);

    apply_section(doc, "sessions", &[
        ScalarField::Bool("confirm_on_close", settings.confirm_on_close),
        ScalarField::Bool("confirm_on_quit", settings.confirm_on_quit),
        ScalarField::Bool("restore_sessions_on_launch", settings.restore_sessions_on_launch),
        ScalarField::EnumStr("group_by", enum_to_string(&settings.group_by)),
    ]);

    apply_worktrees_section(doc, settings);

    apply_claude_section(doc, settings);

    apply_section(doc, "integrations", &[
        ScalarField::OptString("gh_binary_path", settings.gh_binary_path.as_deref()),
    ]);

    apply_section(doc, "notifications", &[
        ScalarField::Bool("notifications_enabled", settings.notifications_enabled),
        ScalarField::Bool("auto_clear_attention_state", settings.auto_clear_attention_state),
    ]);

    apply_section(doc, "keyboard", &[
        ScalarField::Bool("show_pane_hints_on_option", settings.show_pane_hints_on_option),
        ScalarField::Bool("show_session_hints_on_command", settings.show_session_hints_on_command),
    ]);

    apply_section(doc, "advanced", &[
        ScalarField::Bool("enable_logging", settings.enable_logging),
        ScalarField::Bool("update_check_on_launch", settings.update_check_on_launch),
        ScalarField::OptString("default_project_path", settings.default_project_path.as_deref()),
    ]);

    apply_top_level_list(doc, "trusted_workspace", &settings.trusted_workspaces);
    apply_spawn_profiles(doc, &settings.spawn_profiles);
}

/// Field descriptor for [`apply_section`]. Borrows everything; cheap to
/// build inline.
enum ScalarField<'a> {
    String(&'static str, &'a str),
    OptString(&'static str, Option<&'a str>),
    Bool(&'static str, bool),
    U32(&'static str, u32),
    F64(&'static str, f64),
    /// An enum value already stringified via serde_json's camelCase
    /// representation. Pre-stringifying at the call site keeps this enum
    /// from needing a trait object.
    EnumStr(&'static str, String),
}

/// Find or create the section node `name` and apply each scalar field
/// inside its body. Children with names not in `fields` are left alone
/// (preserves user-authored nodes and unknown legacy nodes).
fn apply_section(doc: &mut KdlDocument, section_name: &str, fields: &[ScalarField<'_>]) {
    let section = ensure_section(doc, section_name);
    let body = section.ensure_children();
    for field in fields {
        match field {
            ScalarField::String(name, value) => set_scalar_node(body, name, KdlValue::String((*value).to_string())),
            ScalarField::OptString(name, value) => set_scalar_node(
                body,
                name,
                value.map(|s| KdlValue::String(s.to_string())).unwrap_or(KdlValue::Null),
            ),
            ScalarField::Bool(name, value) => set_scalar_node(body, name, KdlValue::Bool(*value)),
            ScalarField::U32(name, value) => {
                set_scalar_node(body, name, KdlValue::Integer(*value as i128))
            }
            ScalarField::F64(name, value) => set_scalar_node(body, name, KdlValue::Float(*value)),
            ScalarField::EnumStr(name, value) => {
                set_scalar_node(body, name, KdlValue::String(value.clone()));
            }
        }
    }
}

/// Serialize an enum value into the camelCase string the rest of the
/// codebase already uses (matches `#[serde(rename_all = "camelCase")]`).
fn enum_to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("enum serializes as JSON string")
}

/// Worktrees has a list field (`repo_root`) alongside scalars and the
/// legacy bool. Special-cased so we can drop the legacy bool on write.
fn apply_worktrees_section(doc: &mut KdlDocument, settings: &RouxSettings) {
    let section = ensure_section(doc, "worktrees");
    let body = section.ensure_children();

    set_scalar_node(
        body,
        "worktree_base_path",
        settings
            .worktree_base_path
            .as_deref()
            .map(|s| KdlValue::String(s.to_string()))
            .unwrap_or(KdlValue::Null),
    );
    set_enum_node(body, "worktree_cleanup_on_close", &settings.worktree_cleanup_on_close);
    set_scalar_node(
        body,
        "exclude_worktrees_from_repo_roots",
        KdlValue::Bool(settings.exclude_worktrees_from_repo_roots),
    );

    // Drop the legacy bool node entirely. `normalized()` keeps the in-memory
    // bool in sync; readers should consume the enum.
    body.nodes_mut().retain(|n| n.name().value() != "cleanup_worktrees_on_close");

    replace_list_in_body(body, "repo_root", &settings.repo_roots);
}

fn apply_claude_section(doc: &mut KdlDocument, settings: &RouxSettings) {
    let section = ensure_section(doc, "claude");
    let body = section.ensure_children();

    set_scalar_node(
        body,
        "default_model",
        settings
            .default_model
            .as_deref()
            .map(|s| KdlValue::String(s.to_string()))
            .unwrap_or(KdlValue::Null),
    );
    set_scalar_node(
        body,
        "claude_binary_path",
        settings
            .claude_binary_path
            .as_deref()
            .map(|s| KdlValue::String(s.to_string()))
            .unwrap_or(KdlValue::Null),
    );
    replace_list_in_body(body, "additional_flag", &settings.additional_flags);
}

/// Find an existing section by name or append a new empty one. Returns a
/// mutable reference into the document's node list.
fn ensure_section<'a>(doc: &'a mut KdlDocument, name: &str) -> &'a mut KdlNode {
    let pos = doc.nodes().iter().position(|n| n.name().value() == name);
    if let Some(idx) = pos {
        return &mut doc.nodes_mut()[idx];
    }
    let mut node = KdlNode::new(name);
    node.set_children(KdlDocument::new());
    doc.nodes_mut().push(node);
    let last = doc.nodes_mut().len() - 1;
    &mut doc.nodes_mut()[last]
}

/// Set or insert a scalar node `name value` inside `body`. If a node with
/// that name already exists, its first positional entry is replaced; any
/// surrounding leading whitespace / comments survive because we mutate the
/// existing `KdlNode` in place.
fn set_scalar_node(body: &mut KdlDocument, name: &str, value: KdlValue) {
    if let Some(node) = body.nodes_mut().iter_mut().find(|n| n.name().value() == name) {
        replace_first_positional(node, value);
        return;
    }
    let mut node = KdlNode::new(name);
    node.entries_mut().push(KdlEntry::new(value));
    body.nodes_mut().push(node);
}

fn set_enum_node<T: serde::Serialize>(body: &mut KdlDocument, name: &str, value: &T) {
    let json = serde_json::to_value(value).expect("enum serializes");
    let s = json.as_str().expect("enum serializes as JSON string").to_string();
    set_scalar_node(body, name, KdlValue::String(s));
}

fn replace_first_positional(node: &mut KdlNode, value: KdlValue) {
    let pos = node.entries().iter().position(|e| e.name().is_none());
    match pos {
        Some(idx) => {
            // Build a fresh entry so kdl-rs reformats it; the surrounding
            // node trivia (leading whitespace, trailing comment) is on the
            // KdlNode itself and survives.
            node.entries_mut()[idx] = KdlEntry::new(value);
        }
        None => {
            node.entries_mut().push(KdlEntry::new(value));
        }
    }
}

/// Replace every node named `name` inside `body` with a fresh sequence
/// derived from `values`. Used for repeated singular-named list nodes
/// (`repo_root`, `additional_flag`).
fn replace_list_in_body(body: &mut KdlDocument, name: &str, values: &[String]) {
    body.nodes_mut().retain(|n| n.name().value() != name);
    for v in values {
        let mut node = KdlNode::new(name);
        node.entries_mut().push(KdlEntry::new(KdlValue::String(v.clone())));
        body.nodes_mut().push(node);
    }
}

/// Same as [`replace_list_in_body`] but at the top level of the document.
fn apply_top_level_list(doc: &mut KdlDocument, name: &str, values: &[String]) {
    doc.nodes_mut().retain(|n| n.name().value() != name);
    for v in values {
        let mut node = KdlNode::new(name);
        node.entries_mut().push(KdlEntry::new(KdlValue::String(v.clone())));
        doc.nodes_mut().push(node);
    }
}

fn apply_spawn_profiles(doc: &mut KdlDocument, profiles: &[SpawnProfile]) {
    doc.nodes_mut().retain(|n| n.name().value() != "spawn_profile");
    for p in profiles {
        doc.nodes_mut().push(render_spawn_profile(p));
    }
}

fn render_spawn_profile(p: &SpawnProfile) -> KdlNode {
    let mut node = KdlNode::new("spawn_profile");

    push_attr(&mut node, "id", KdlValue::String(p.id.clone()));
    push_attr(&mut node, "name", KdlValue::String(p.name.clone()));
    if let Some(icon) = &p.icon {
        push_attr(&mut node, "icon", KdlValue::String(icon.clone()));
    }
    if let Some(provider) = &p.provider {
        let s = serde_json::to_value(provider).ok().and_then(|v| v.as_str().map(str::to_string));
        if let Some(s) = s {
            push_attr(&mut node, "provider", KdlValue::String(s));
        }
    }
    if let Some(nono_profile) = &p.nono_profile {
        push_attr(&mut node, "nono_profile", KdlValue::String(nono_profile.clone()));
    }

    let has_body = p.setup_command.is_some()
        || p.startup_command.is_some()
        || p.startup_behavior.is_some()
        || p.cwd_override.is_some()
        || p.env.as_ref().map(|m| !m.is_empty()).unwrap_or(false)
        || p.nono_allow_dirs.as_ref().map(|d| !d.is_empty()).unwrap_or(false);

    if has_body {
        let mut body = KdlDocument::new();
        if let Some(s) = &p.setup_command {
            set_scalar_node(&mut body, "setup_command", KdlValue::String(s.clone()));
        }
        if let Some(s) = &p.startup_command {
            set_scalar_node(&mut body, "startup_command", KdlValue::String(s.clone()));
        }
        if let Some(b) = &p.startup_behavior {
            set_enum_node(&mut body, "startup_behavior", b);
        }
        if let Some(s) = &p.cwd_override {
            set_scalar_node(&mut body, "cwd_override", KdlValue::String(s.clone()));
        }
        if let Some(env) = &p.env {
            if !env.is_empty() {
                let mut env_node = KdlNode::new("env");
                let mut env_body = KdlDocument::new();
                for (k, v) in env {
                    let mut kv = KdlNode::new(k.as_str());
                    kv.entries_mut().push(KdlEntry::new(KdlValue::String(v.clone())));
                    env_body.nodes_mut().push(kv);
                }
                env_node.set_children(env_body);
                body.nodes_mut().push(env_node);
            }
        }
        if let Some(dirs) = &p.nono_allow_dirs {
            for d in dirs {
                let mut n = KdlNode::new("nono_allow_dir");
                n.entries_mut().push(KdlEntry::new(KdlValue::String(d.clone())));
                body.nodes_mut().push(n);
            }
        }
        node.set_children(body);
    }

    node
}

fn push_attr(node: &mut KdlNode, name: &str, value: KdlValue) {
    let ident = kdl::KdlIdentifier::from_str(name).expect("static attr names are valid identifiers");
    let mut entry = KdlEntry::new(value);
    entry.set_name(Some(ident));
    node.entries_mut().push(entry);
}

// ---------------------------------------------------------------------------
// span helpers (mirrored from layout.rs)
// ---------------------------------------------------------------------------

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

fn node_loc(src: &str, node: &KdlNode) -> (usize, usize) {
    offset_to_line_col(src, node.span().offset())
}

fn entry_loc(src: &str, entry: &KdlEntry) -> (usize, usize) {
    offset_to_line_col(src, entry.span().offset())
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::settings::{
        CursorStyle, GroupBy, StatusBarPosition, TabPosition, WorktreeCleanupMode,
    };

    fn sample_settings() -> RouxSettings {
        RouxSettings {
            theme: "nordic-night".to_string(),
            tab_width: 320,
            font_size: 16,
            cursor_style: CursorStyle::Bar,
            cursor_blink: false,
            tab_position: TabPosition::Right,
            status_bar_position: StatusBarPosition::Top,
            group_by: GroupBy::Project,
            worktree_cleanup_on_close: WorktreeCleanupMode::Always,
            // `cleanup_worktrees_on_close` is intentionally not pre-set
            // here — it's the legacy field that `apply` drops on write,
            // and `normalized()` re-derives it from the enum on load.
            // Including it in the sample would fail the round-trip on a
            // detail the design deliberately discards.
            repo_roots: vec!["/tmp/a".to_string(), "/tmp/b".to_string()],
            additional_flags: vec!["--verbose".to_string(), "--debug".to_string()],
            trusted_workspaces: vec!["/work/one".to_string()],
            claude_binary_path: Some("/usr/local/bin/claude".to_string()),
            gh_binary_path: Some("/opt/homebrew/bin/gh".to_string()),
            default_model: Some("opus".to_string()),
            default_project_path: Some("/projects".to_string()),
            worktree_base_path: Some("/wt".to_string()),
            ..RouxSettings::default()
        }
    }

    #[test]
    fn render_default_round_trips_to_default_settings() {
        let rendered = render_default();
        let parsed = parse(&rendered).expect("default round trip parses");
        assert_eq!(parsed, RouxSettings::default());
    }

    #[test]
    fn round_trip_preserves_non_default_values() {
        let original = sample_settings();
        let rendered = apply("", &original).expect("apply onto empty");
        let parsed = parse(&rendered).expect("round trip parses");
        // Compare normalized forms so the legacy `cleanup_worktrees_on_close`
        // bool — derived from the enum on load — is consistent on both sides.
        assert_eq!(parsed.normalized(), original.normalized());
    }

    #[test]
    fn parse_accepts_legacy_cleanup_bool() {
        // A hand-edited KDL that still uses the legacy boolean must
        // deserialize. `normalized` then promotes it to the enum.
        // KDL v2 spells boolean keywords with a leading `#`.
        let input = r#"
            worktrees {
                cleanup_worktrees_on_close #true
            }
        "#;
        let parsed = parse(input).expect("legacy bool parses");
        assert!(parsed.cleanup_worktrees_on_close);
        let normalized = parsed.normalized();
        assert_eq!(normalized.worktree_cleanup_on_close, WorktreeCleanupMode::Always);
        assert!(normalized.cleanup_worktrees_on_close);
    }

    #[test]
    fn parse_ignores_unknown_top_level_nodes() {
        let input = r#"
            mystery {
                foo "bar"
            }
            ui {
                theme "deep-blue"
            }
        "#;
        let parsed = parse(input).expect("unknown nodes ignored");
        assert_eq!(parsed.theme, "deep-blue");
    }

    #[test]
    fn parse_ignores_unknown_section_children() {
        let input = r#"
            ui {
                theme "deep-blue"
                future_field 42
            }
        "#;
        let parsed = parse(input).expect("unknown section children ignored");
        assert_eq!(parsed.theme, "deep-blue");
    }

    #[test]
    fn parse_reports_line_column_on_syntax_error() {
        let input = "ui {\n  theme \"unterminated\n}";
        let err = parse(input).expect_err("malformed input");
        match err {
            SettingsKdlError::Syntax { line, .. } => {
                assert!(line >= 2, "expected error on line >= 2, got {line}");
            }
            other => panic!("expected Syntax error, got {other:?}"),
        }
    }

    #[test]
    fn apply_preserves_top_level_comments() {
        let input = "// keep me\nui {\n    theme \"deep-blue\"\n}\n";
        let mut s = RouxSettings::default();
        s.theme = "graphite-rose".to_string();
        let out = apply(input, &s).expect("apply succeeds");
        assert!(out.contains("// keep me"), "top-level comment lost: {out}");
        assert!(out.contains("graphite-rose"), "value not applied: {out}");
    }

    #[test]
    fn apply_preserves_section_comments() {
        let input = "ui {\n    // a comment about theme\n    theme \"deep-blue\"\n}\n";
        let mut s = RouxSettings::default();
        s.theme = "mocha-soft".to_string();
        let out = apply(input, &s).expect("apply succeeds");
        assert!(out.contains("// a comment about theme"), "section comment lost: {out}");
        assert!(out.contains("mocha-soft"), "value not applied: {out}");
    }

    #[test]
    fn apply_replaces_list_atomically() {
        // A pre-existing repo_root list gets fully replaced from the struct,
        // not appended-to.
        let input = r#"
            worktrees {
                repo_root "/old/one"
                repo_root "/old/two"
                repo_root "/old/three"
            }
        "#;
        let mut s = RouxSettings::default();
        s.repo_roots = vec!["/new/only".to_string()];
        let out = apply(input, &s).expect("apply succeeds");
        let parsed = parse(&out).unwrap();
        assert_eq!(parsed.repo_roots, vec!["/new/only".to_string()]);
        assert!(!out.contains("/old/one"));
    }

    #[test]
    fn apply_strips_legacy_cleanup_bool_on_write() {
        let input = r#"
            worktrees {
                cleanup_worktrees_on_close #true
            }
        "#;
        let s = RouxSettings::default();
        let out = apply(input, &s).expect("apply succeeds");
        assert!(
            !out.contains("cleanup_worktrees_on_close"),
            "legacy bool should be stripped on write: {out}",
        );
        assert!(out.contains("worktree_cleanup_on_close"), "enum should be present: {out}");
    }

    #[test]
    fn apply_creates_missing_section() {
        // Input has only `terminal`; applying a full struct should add the
        // other sections without removing `terminal`.
        let input = "terminal {\n    font_size 18\n}\n";
        let mut s = RouxSettings::default();
        s.font_size = 18;
        s.theme = "paper-ink".to_string();
        let out = apply(input, &s).expect("apply succeeds");
        let parsed = parse(&out).unwrap();
        assert_eq!(parsed.font_size, 18);
        assert_eq!(parsed.theme, "paper-ink");
    }

    #[test]
    fn render_default_is_sane_looking() {
        // Smoke-test the formatting of the seed document. If this
        // ever regresses to a single-line dump or loses sections,
        // hand-editability dies — flag it loudly.
        let s = render_default();
        assert!(s.starts_with("// Roux settings."), "missing header comment:\n{s}");
        for section in ["ui {", "terminal {", "sessions {", "worktrees {", "claude {", "integrations {", "notifications {", "keyboard {", "advanced {"] {
            assert!(s.contains(section), "missing section `{section}` in:\n{s}");
        }
    }

    #[test]
    fn parse_handles_all_enum_variants() {
        for cursor in [CursorStyle::Block, CursorStyle::Underline, CursorStyle::Bar] {
            let mut s = RouxSettings::default();
            s.cursor_style = cursor.clone();
            let rendered = apply("", &s).unwrap();
            let parsed = parse(&rendered).unwrap();
            assert_eq!(parsed.cursor_style, cursor);
        }
        for tab in [TabPosition::Left, TabPosition::Right] {
            let mut s = RouxSettings::default();
            s.tab_position = tab.clone();
            let rendered = apply("", &s).unwrap();
            let parsed = parse(&rendered).unwrap();
            assert_eq!(parsed.tab_position, tab);
        }
        for mode in [WorktreeCleanupMode::Never, WorktreeCleanupMode::Prompt, WorktreeCleanupMode::Always] {
            let mut s = RouxSettings::default();
            s.worktree_cleanup_on_close = mode;
            let rendered = apply("", &s).unwrap();
            let parsed = parse(&rendered).unwrap();
            assert_eq!(parsed.worktree_cleanup_on_close, mode);
        }
        for sb in [StatusBarPosition::Top, StatusBarPosition::Bottom] {
            let mut s = RouxSettings::default();
            s.status_bar_position = sb.clone();
            let rendered = apply("", &s).unwrap();
            let parsed = parse(&rendered).unwrap();
            assert_eq!(parsed.status_bar_position, sb);
        }
        for gb in [GroupBy::Repo, GroupBy::Project] {
            let mut s = RouxSettings::default();
            s.group_by = gb.clone();
            let rendered = apply("", &s).unwrap();
            let parsed = parse(&rendered).unwrap();
            assert_eq!(parsed.group_by, gb);
        }
    }

    #[test]
    fn null_optional_fields_round_trip() {
        // Defaults have several `Option<String>` set to None — they must
        // render as `null` and parse back as None.
        let s = RouxSettings::default();
        let rendered = apply("", &s).unwrap();
        let parsed = parse(&rendered).unwrap();
        assert_eq!(parsed.claude_binary_path, None);
        assert_eq!(parsed.gh_binary_path, None);
        assert_eq!(parsed.worktree_base_path, None);
        assert_eq!(parsed.default_model, None);
        assert_eq!(parsed.default_project_path, None);
    }

    #[test]
    fn spawn_profile_round_trip() {
        use std::collections::BTreeMap;
        let mut env = BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());

        let mut profile = SpawnProfile::builtin("custom-claude", "Custom Claude");
        profile.startup_command = Some("claude --model opus".to_string());
        profile.startup_behavior = Some(StartupBehavior::AutoRun);
        profile.provider = Some(Provider::Claude);
        profile.env = Some(env);
        profile.icon = Some("sparkle".to_string());
        // Must be User after round trip — parse forces it.
        profile.source = ProfileSource::User;

        let mut s = RouxSettings::default();
        s.spawn_profiles = vec![profile.clone()];
        let rendered = apply("", &s).unwrap();
        let parsed = parse(&rendered).unwrap();
        assert_eq!(parsed.spawn_profiles.len(), 1);
        assert_eq!(parsed.spawn_profiles[0], profile);
    }
}
