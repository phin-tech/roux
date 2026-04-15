//! Roux keymap v1 — types and KDL parser.
//!
//! A [`ParsedKeymap`] is the fully-resolved, runtime-ready shape produced
//! from a single `keymap.kdl` document. This module handles syntactic
//! parsing and structural validation (key notation, tree references,
//! attribute names) but does NOT:
//!
//! - Merge presets. The loader parses the preset KDL separately and
//!   overlays the user document via [`merge_keymaps`].
//! - Validate command IDs against the registry. The frontend knows which
//!   command IDs exist; it validates after receiving the parsed shape.
//!
//! This is the only place in the crate that touches `kdl`. Callers see
//! plain Rust types.
//!
//! See `docs/superpowers/specs/2026-04-14-configurable-keymap-design.md`
//! for the full schema.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// public types
// ---------------------------------------------------------------------------

/// A modifier key. `Cmd` is platform-dispatched: on macOS it matches Meta,
/// elsewhere it matches Ctrl. The resolver decides which physical flag to
/// compare against at match time; the stored form is platform-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Modifier {
    Cmd,
    Ctrl,
    Alt,
    Shift,
}

/// How a bound key is matched against a `KeyboardEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum KeyRef {
    /// Match `event.code` — survives keyboard-layout quirks (macOS Option
    /// producing `∆` for `j`, etc.). Used for any binding with a modifier
    /// prefix: `Alt+KeyH`, `Cmd+Digit1`, `Ctrl+KeyB`.
    Physical { mods: Vec<Modifier>, code: String },
    /// Match `event.key` — the logical character after Shift/dead-keys.
    /// Used for bare bindings inside trees: `"h"`, `"%"`, `"Escape"`.
    Character { mods: Vec<Modifier>, key: String },
}

/// What a bind fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum KeymapAction {
    /// Execute a registered command by id.
    Command { id: String },
    /// Promote to a named tree (drill-down within a chord sequence).
    EnterTree { tree: String },
}

/// HUD visibility for a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HudMode {
    Always,
    Delayed { ms: u32 },
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Bind {
    pub key: KeyRef,
    pub action: KeymapAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KeymapTree {
    pub name: String,
    #[serde(default)]
    pub sticky: bool,
    #[serde(default)]
    pub passthrough: bool,
    #[serde(default)]
    pub hud: Option<HudMode>,
    pub binds: Vec<Bind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Prefix {
    pub key: KeyRef,
    pub tree: String,
}

/// A non-fatal issue discovered while parsing. Warnings surface through the
/// notification system; the keymap still loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KeymapWarning {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

/// Fully parsed keymap. `preset_ref` is the raw `preset "<name>"`
/// reference, if the document declared one; the loader resolves it by
/// parsing the preset KDL separately and calling [`merge_keymaps`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ParsedKeymap {
    #[serde(default)]
    pub preset_ref: Option<String>,
    pub hud_default: HudMode,
    pub direct_binds: Vec<Bind>,
    /// Keys to drop from the base when merging a preset; only meaningful on
    /// the "user overlay" side of a merge.
    pub unbinds: Vec<KeyRef>,
    pub trees: Vec<KeymapTree>,
    pub prefixes: Vec<Prefix>,
    pub warnings: Vec<KeymapWarning>,
}

impl Default for ParsedKeymap {
    fn default() -> Self {
        Self {
            preset_ref: None,
            hud_default: HudMode::Always,
            direct_binds: Vec::new(),
            unbinds: Vec::new(),
            trees: Vec::new(),
            prefixes: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum KeymapParseError {
    #[error("kdl parse error at {line}:{column}: {message}")]
    Syntax { line: u32, column: u32, message: String },
    #[error("{message} at {line}:{column}")]
    Schema { line: u32, column: u32, message: String },
}

impl KeymapParseError {
    fn schema(loc: (u32, u32), msg: impl Into<String>) -> Self {
        Self::Schema {
            line: loc.0,
            column: loc.1,
            message: msg.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------

/// Parse a `keymap.kdl` document. Returns a fully-shaped [`ParsedKeymap`];
/// warnings (duplicate prefixes, prefix/bind collisions) accumulate on the
/// result rather than erroring out. Command-ID validation lives on the
/// frontend and is layered on top.
pub fn parse_keymap_kdl(src: &str) -> Result<ParsedKeymap, KeymapParseError> {
    let doc: kdl::KdlDocument = src.parse().map_err(|e: kdl::KdlError| {
        let (line, column, message) = e
            .diagnostics
            .first()
            .map(|d| {
                let (l, c) = offset_to_line_col(src, d.span.offset());
                (
                    l,
                    c,
                    d.message.clone().unwrap_or_else(|| "invalid KDL syntax".into()),
                )
            })
            .unwrap_or_else(|| (0, 0, "invalid KDL syntax".into()));
        KeymapParseError::Syntax {
            line,
            column,
            message,
        }
    })?;

    let mut km = ParsedKeymap::default();
    let mut seen_prefix_triggers: Vec<(KeyRef, u32, u32)> = Vec::new();
    let mut seen_direct_binds: Vec<(KeyRef, u32, u32)> = Vec::new();

    for node in doc.nodes() {
        let name = node.name().value();
        let loc = node_loc(src, node);

        match name {
            "preset" => {
                if km.preset_ref.is_some() {
                    return Err(KeymapParseError::schema(
                        loc,
                        "duplicate `preset` declaration",
                    ));
                }
                km.preset_ref = Some(single_string_arg(src, node, "preset")?);
            }
            "hud" => {
                km.hud_default = parse_hud_mode(src, node)?;
            }
            "bind" => {
                let (key, action) = parse_top_level_bind(src, node)?;
                if let Some(prev) = seen_direct_binds.iter().find(|(k, _, _)| *k == key) {
                    km.warnings.push(KeymapWarning {
                        message: format!(
                            "duplicate direct bind for `{}` — earlier occurrence at {}:{} replaced",
                            format_keyref(&key),
                            prev.1,
                            prev.2
                        ),
                        line: loc.0,
                        column: loc.1,
                    });
                    km.direct_binds.retain(|b| b.key != key);
                }
                seen_direct_binds.retain(|(k, _, _)| *k != key);
                seen_direct_binds.push((key.clone(), loc.0, loc.1));
                km.direct_binds.push(Bind { key, action });
            }
            "unbind" => {
                let key = parse_key_arg(src, node, BindContext::TopLevel)?;
                km.unbinds.push(key);
            }
            "tree" => {
                let tree = parse_tree(src, node)?;
                if let Some(existing) = km.trees.iter().position(|t| t.name == tree.name) {
                    km.trees[existing] = tree;
                } else {
                    km.trees.push(tree);
                }
            }
            "prefix" => {
                let (key, tree_name) = parse_prefix(src, node)?;
                if let Some(prev) = seen_prefix_triggers.iter().find(|(k, _, _)| *k == key) {
                    km.warnings.push(KeymapWarning {
                        message: format!(
                            "duplicate prefix trigger `{}` — earlier occurrence at {}:{} kept",
                            format_keyref(&key),
                            prev.1,
                            prev.2
                        ),
                        line: loc.0,
                        column: loc.1,
                    });
                    continue;
                }
                seen_prefix_triggers.push((key.clone(), loc.0, loc.1));
                km.prefixes.push(Prefix {
                    key,
                    tree: tree_name,
                });
            }
            other => {
                return Err(KeymapParseError::schema(
                    loc,
                    format!("unknown top-level node `{other}`"),
                ));
            }
        }
    }

    // Collision check: prefix triggers vs direct binds.
    for prefix in &km.prefixes {
        if km.direct_binds.iter().any(|b| b.key == prefix.key) {
            km.direct_binds.retain(|b| b.key != prefix.key);
            km.warnings.push(KeymapWarning {
                message: format!(
                    "direct bind `{}` shadowed by prefix; bind dropped",
                    format_keyref(&prefix.key)
                ),
                line: 0,
                column: 0,
            });
        }
    }

    Ok(km)
}

/// Merge a user overlay on top of a base keymap (usually a preset).
///
/// Composition semantics match the spec:
/// - `direct_binds` on the same key replace.
/// - `unbinds` drop matching direct binds from the base.
/// - `trees` replace by name (whole tree, no per-bind merge).
/// - `prefixes` on the same trigger replace.
/// - `hud_default` from the overlay wins if the overlay set one; otherwise
///   the base's default carries over. (The parser always sets
///   `hud_default`; we detect "overlay did not set hud" by the overlay
///   document not containing a `hud` node, which requires an extra
///   parameter. For simplicity in v1, overlay always wins — users who
///   want to inherit just don't declare `hud`. A future change can add a
///   sentinel.)
/// - Warnings from both sides are concatenated.
pub fn merge_keymaps(base: ParsedKeymap, overlay: ParsedKeymap) -> ParsedKeymap {
    let mut out = base;

    // unbind from base.
    for key in &overlay.unbinds {
        out.direct_binds.retain(|b| b.key != *key);
    }

    // direct binds: overlay replaces by key.
    for bind in overlay.direct_binds {
        out.direct_binds.retain(|b| b.key != bind.key);
        out.direct_binds.push(bind);
    }

    // trees: replace by name.
    for tree in overlay.trees {
        if let Some(idx) = out.trees.iter().position(|t| t.name == tree.name) {
            out.trees[idx] = tree;
        } else {
            out.trees.push(tree);
        }
    }

    // prefixes: replace by key.
    for prefix in overlay.prefixes {
        out.prefixes.retain(|p| p.key != prefix.key);
        out.prefixes.push(prefix);
    }

    out.hud_default = overlay.hud_default;
    out.preset_ref = overlay.preset_ref.or(out.preset_ref);
    out.warnings.extend(overlay.warnings);
    out
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

#[derive(Copy, Clone)]
enum BindContext {
    TopLevel,
    Tree,
}

fn parse_tree(src: &str, node: &kdl::KdlNode) -> Result<KeymapTree, KeymapParseError> {
    let name_entry = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .ok_or_else(|| {
            KeymapParseError::schema(
                node_loc(src, node),
                "`tree` requires a name argument, e.g. `tree \"leader\"`",
            )
        })?;
    let name = name_entry
        .value()
        .as_string()
        .ok_or_else(|| {
            KeymapParseError::schema(
                entry_loc(src, name_entry),
                "tree name must be a string",
            )
        })?
        .to_string();

    // Trees take no attributes (`sticky`, `passthrough`, `hud` live as child
    // nodes inside the body). Reject attributes so typos surface instead of
    // silently parsing.
    if let Some(attr) = node.entries().iter().find(|e| e.name().is_some()) {
        return Err(KeymapParseError::schema(
            entry_loc(src, attr),
            format!(
                "`tree` takes no attributes; put `{}` inside the body instead",
                attr.name().unwrap().value()
            ),
        ));
    }

    let children = node.children().ok_or_else(|| {
        KeymapParseError::schema(node_loc(src, node), "`tree` must have a body `{ ... }`")
    })?;

    let mut sticky = false;
    let mut passthrough = false;
    let mut hud: Option<HudMode> = None;
    let mut binds = Vec::new();

    for child in children.nodes() {
        match child.name().value() {
            "sticky" => sticky = true,
            "passthrough" => passthrough = true,
            "hud" => hud = Some(parse_hud_mode(src, child)?),
            "bind" => {
                let (key, action) = parse_tree_bind(src, child)?;
                binds.push(Bind { key, action });
            }
            other => {
                return Err(KeymapParseError::schema(
                    node_loc(src, child),
                    format!("unknown node `{other}` inside `tree`"),
                ));
            }
        }
    }

    Ok(KeymapTree {
        name,
        sticky,
        passthrough,
        hud,
        binds,
    })
}

fn parse_top_level_bind(
    src: &str,
    node: &kdl::KdlNode,
) -> Result<(KeyRef, KeymapAction), KeymapParseError> {
    let positional: Vec<&kdl::KdlEntry> =
        node.entries().iter().filter(|e| e.name().is_none()).collect();
    if positional.len() < 2 {
        return Err(KeymapParseError::schema(
            node_loc(src, node),
            "`bind` requires a key and a command id, e.g. `bind \"Cmd+KeyK\" \"app.command-palette\"`",
        ));
    }
    let key_entry = positional[0];
    let key_str = key_entry.value().as_string().ok_or_else(|| {
        KeymapParseError::schema(entry_loc(src, key_entry), "bind key must be a string")
    })?;
    let key = parse_key_string(entry_loc(src, key_entry), key_str, BindContext::TopLevel)?;

    let cmd_entry = positional[1];
    let cmd = cmd_entry.value().as_string().ok_or_else(|| {
        KeymapParseError::schema(entry_loc(src, cmd_entry), "command id must be a string")
    })?;

    Ok((key, KeymapAction::Command { id: cmd.to_string() }))
}

fn parse_tree_bind(
    src: &str,
    node: &kdl::KdlNode,
) -> Result<(KeyRef, KeymapAction), KeymapParseError> {
    let positional: Vec<&kdl::KdlEntry> =
        node.entries().iter().filter(|e| e.name().is_none()).collect();
    if positional.is_empty() {
        return Err(KeymapParseError::schema(
            node_loc(src, node),
            "`bind` requires at least a key argument",
        ));
    }
    let key_entry = positional[0];
    let key_str = key_entry.value().as_string().ok_or_else(|| {
        KeymapParseError::schema(entry_loc(src, key_entry), "bind key must be a string")
    })?;
    let key = parse_key_string(entry_loc(src, key_entry), key_str, BindContext::Tree)?;

    // Two forms:
    //   bind "h" "pane.focus-left"
    //   bind "w" { enter-tree "leader-panes" }
    if positional.len() >= 2 {
        let cmd_entry = positional[1];
        let cmd = cmd_entry.value().as_string().ok_or_else(|| {
            KeymapParseError::schema(entry_loc(src, cmd_entry), "command id must be a string")
        })?;
        return Ok((key, KeymapAction::Command { id: cmd.to_string() }));
    }

    let children = node.children().ok_or_else(|| {
        KeymapParseError::schema(
            node_loc(src, node),
            "`bind` needs either a command-id argument or an `enter-tree` block",
        )
    })?;
    let action_nodes: Vec<&kdl::KdlNode> = children.nodes().iter().collect();
    if action_nodes.len() != 1 {
        return Err(KeymapParseError::schema(
            node_loc(src, node),
            "`bind` block must contain exactly one action node (e.g. `enter-tree`)",
        ));
    }
    let action_node = action_nodes[0];
    match action_node.name().value() {
        "enter-tree" => {
            let tree = single_string_arg(src, action_node, "enter-tree")?;
            Ok((key, KeymapAction::EnterTree { tree }))
        }
        other => Err(KeymapParseError::schema(
            node_loc(src, action_node),
            format!("unknown action `{other}` inside `bind` block"),
        )),
    }
}

fn parse_prefix(src: &str, node: &kdl::KdlNode) -> Result<(KeyRef, String), KeymapParseError> {
    let key_entry = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .ok_or_else(|| {
            KeymapParseError::schema(
                node_loc(src, node),
                "`prefix` requires a key, e.g. `prefix \"Ctrl+KeyB\" tree=\"tmux\"`",
            )
        })?;
    let key_str = key_entry.value().as_string().ok_or_else(|| {
        KeymapParseError::schema(entry_loc(src, key_entry), "prefix key must be a string")
    })?;
    let key = parse_key_string(entry_loc(src, key_entry), key_str, BindContext::TopLevel)?;

    let tree_entry = node
        .entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some("tree"))
        .ok_or_else(|| {
            KeymapParseError::schema(
                node_loc(src, node),
                "`prefix` requires a `tree=\"<name>\"` attribute",
            )
        })?;
    let tree_name = tree_entry
        .value()
        .as_string()
        .ok_or_else(|| {
            KeymapParseError::schema(entry_loc(src, tree_entry), "`tree` attribute must be a string")
        })?
        .to_string();

    Ok((key, tree_name))
}

fn parse_key_arg(
    src: &str,
    node: &kdl::KdlNode,
    context: BindContext,
) -> Result<KeyRef, KeymapParseError> {
    let entry = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .ok_or_else(|| {
            KeymapParseError::schema(
                node_loc(src, node),
                format!("`{}` requires a key argument", node.name().value()),
            )
        })?;
    let s = entry.value().as_string().ok_or_else(|| {
        KeymapParseError::schema(entry_loc(src, entry), "key must be a string")
    })?;
    parse_key_string(entry_loc(src, entry), s, context)
}

fn parse_hud_mode(src: &str, node: &kdl::KdlNode) -> Result<HudMode, KeymapParseError> {
    let entry = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .ok_or_else(|| {
            KeymapParseError::schema(
                node_loc(src, node),
                "`hud` requires a mode string, e.g. `hud \"always\"`",
            )
        })?;
    let s = entry.value().as_string().ok_or_else(|| {
        KeymapParseError::schema(entry_loc(src, entry), "`hud` value must be a string")
    })?;
    parse_hud_string(entry_loc(src, entry), s)
}

fn parse_hud_string(loc: (u32, u32), s: &str) -> Result<HudMode, KeymapParseError> {
    let s = s.trim();
    if s == "always" {
        return Ok(HudMode::Always);
    }
    if s == "never" {
        return Ok(HudMode::Never);
    }
    if let Some(rest) = s.strip_prefix("delayed") {
        let ms_str = rest.trim();
        if let Ok(ms) = ms_str.parse::<u32>() {
            return Ok(HudMode::Delayed { ms });
        }
    }
    Err(KeymapParseError::schema(
        loc,
        format!("invalid hud mode `{s}`; expected `always`, `never`, or `delayed <ms>`"),
    ))
}

fn single_string_arg(
    src: &str,
    node: &kdl::KdlNode,
    label: &str,
) -> Result<String, KeymapParseError> {
    let entry = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .ok_or_else(|| {
            KeymapParseError::schema(
                node_loc(src, node),
                format!("`{label}` requires a string argument"),
            )
        })?;
    entry
        .value()
        .as_string()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            KeymapParseError::schema(
                entry_loc(src, entry),
                format!("`{label}` argument must be a string"),
            )
        })
}

// ---------------------------------------------------------------------------
// key parsing
// ---------------------------------------------------------------------------

/// Parse a key string like `"Cmd+KeyK"`, `"C-b"`, `"M-Left"`, `"h"`, `"%"`.
///
/// Tmux-style aliases `C-`, `M-`, `S-` are normalized to `Ctrl+`, `Alt+`,
/// `Shift+` up front. The trailing token determines physical-vs-character:
///
/// - A token matching `Key[A-Z]`, `Digit[0-9]`, or a recognized named-key
///   code (`ArrowLeft`, `Escape`, `Tab`, …) resolves to [`KeyRef::Physical`].
/// - A bare single character (`h`, `%`) resolves to [`KeyRef::Character`].
/// - A named key like `Escape` or `ArrowLeft` resolves to
///   [`KeyRef::Physical`] with the same string, which matches both
///   `event.code === "Escape"` and `event.key === "Escape"` on the frontend
///   (the resolver treats these named keys as physical because they have no
///   shifted variant).
///
/// Any binding with a modifier prefix defaults to physical notation. A bare
/// single-character-with-modifier like `Alt+h` gets its trailing `h`
/// promoted to `KeyH`.
fn parse_key_string(
    loc: (u32, u32),
    input: &str,
    _context: BindContext,
) -> Result<KeyRef, KeymapParseError> {
    if input.is_empty() {
        return Err(KeymapParseError::schema(loc, "empty key string"));
    }

    // Normalize tmux-style aliases.
    let normalized = normalize_aliases(input);
    let tokens: Vec<&str> = normalized.split('+').map(|t| t.trim()).collect();
    if tokens.is_empty() || tokens.iter().any(|t| t.is_empty()) {
        return Err(KeymapParseError::schema(
            loc,
            format!("invalid key string `{input}`"),
        ));
    }
    let (mod_tokens, key_token) = tokens.split_at(tokens.len() - 1);
    let key_token = key_token[0];

    let mut mods = Vec::new();
    for t in mod_tokens {
        let m = match *t {
            "Cmd" => Modifier::Cmd,
            "Ctrl" => Modifier::Ctrl,
            "Alt" => Modifier::Alt,
            "Shift" => Modifier::Shift,
            other => {
                return Err(KeymapParseError::schema(
                    loc,
                    format!("unknown modifier `{other}`"),
                ))
            }
        };
        if !mods.contains(&m) {
            mods.push(m);
        }
    }
    sort_mods(&mut mods);

    let has_mods = !mods.is_empty();

    // Physical-notation markers.
    let is_physical_token = is_physical_code(key_token);

    // Character-notation: bare character with no modifiers.
    if !has_mods && !is_physical_token {
        if key_token.chars().count() == 1 {
            return Ok(KeyRef::Character {
                mods,
                key: key_token.to_string(),
            });
        }
        // Named keys like "Escape", "Tab", "Space" act as physical codes
        // on both e.code and e.key — store as Physical with the canonical
        // name; the resolver compares against e.code OR e.key.
        if is_named_key(key_token) {
            return Ok(KeyRef::Physical {
                mods,
                code: key_token.to_string(),
            });
        }
        return Err(KeymapParseError::schema(
            loc,
            format!("unrecognized key `{key_token}`"),
        ));
    }

    // With modifiers or explicit physical code → physical notation.
    let code = if is_physical_token || is_named_key(key_token) {
        key_token.to_string()
    } else if key_token.chars().count() == 1 {
        // Promote `Alt+h` → `Alt+KeyH`, `Cmd+1` → `Cmd+Digit1`, `Cmd+;` → `Cmd+Semicolon`.
        promote_char_to_code(loc, key_token)?
    } else {
        return Err(KeymapParseError::schema(
            loc,
            format!("unrecognized physical key `{key_token}`"),
        ));
    };

    Ok(KeyRef::Physical { mods, code })
}

fn normalize_aliases(input: &str) -> String {
    // Replace C-/M-/S- prefixes at the start of each `+`-separated token.
    let mut out = String::with_capacity(input.len());
    let mut start_of_token = true;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if start_of_token {
            if c == 'C' && chars.peek() == Some(&'-') {
                out.push_str("Ctrl+");
                chars.next();
                start_of_token = false;
                continue;
            }
            if c == 'M' && chars.peek() == Some(&'-') {
                out.push_str("Alt+");
                chars.next();
                start_of_token = false;
                continue;
            }
            if c == 'S' && chars.peek() == Some(&'-') {
                out.push_str("Shift+");
                chars.next();
                start_of_token = false;
                continue;
            }
        }
        out.push(c);
        start_of_token = c == '+';
    }
    // Arrow-key aliases: tmux users write `Left`/`Right`/`Up`/`Down`.
    // Rewrite the trailing token if it matches one of these, being careful
    // not to touch substrings of other tokens.
    rewrite_trailing_arrow_alias(&out)
}

fn rewrite_trailing_arrow_alias(s: &str) -> String {
    let split_at = s.rfind('+').map(|i| i + 1).unwrap_or(0);
    let (prefix, tail) = s.split_at(split_at);
    let replacement = match tail {
        "Left" => Some("ArrowLeft"),
        "Right" => Some("ArrowRight"),
        "Up" => Some("ArrowUp"),
        "Down" => Some("ArrowDown"),
        _ => None,
    };
    match replacement {
        Some(new_tail) => format!("{prefix}{new_tail}"),
        None => s.to_string(),
    }
}

fn is_physical_code(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix("Key") {
        return rest.len() == 1 && rest.chars().next().unwrap().is_ascii_uppercase();
    }
    if let Some(rest) = s.strip_prefix("Digit") {
        return rest.len() == 1 && rest.chars().next().unwrap().is_ascii_digit();
    }
    false
}

fn is_named_key(s: &str) -> bool {
    matches!(
        s,
        "Escape"
            | "Tab"
            | "Space"
            | "Enter"
            | "Backspace"
            | "Delete"
            | "Home"
            | "End"
            | "PageUp"
            | "PageDown"
            | "ArrowUp"
            | "ArrowDown"
            | "ArrowLeft"
            | "ArrowRight"
            | "Semicolon"
            | "Comma"
            | "Period"
            | "Slash"
            | "Backslash"
            | "Quote"
            | "BracketLeft"
            | "BracketRight"
            | "Minus"
            | "Equal"
            | "Backquote"
            | "F1"
            | "F2"
            | "F3"
            | "F4"
            | "F5"
            | "F6"
            | "F7"
            | "F8"
            | "F9"
            | "F10"
            | "F11"
            | "F12"
    )
}

fn promote_char_to_code(loc: (u32, u32), c: &str) -> Result<String, KeymapParseError> {
    let ch = c.chars().next().unwrap();
    if ch.is_ascii_alphabetic() {
        return Ok(format!("Key{}", ch.to_ascii_uppercase()));
    }
    if ch.is_ascii_digit() {
        return Ok(format!("Digit{ch}"));
    }
    let name = match ch {
        ';' => "Semicolon",
        ',' => "Comma",
        '.' => "Period",
        '/' => "Slash",
        '\\' => "Backslash",
        '\'' => "Quote",
        '[' => "BracketLeft",
        ']' => "BracketRight",
        '-' => "Minus",
        '=' => "Equal",
        '`' => "Backquote",
        _ => {
            return Err(KeymapParseError::schema(
                loc,
                format!(
                    "cannot promote character `{c}` to a physical code; use explicit code like `KeyX` or `Digit1`"
                ),
            ))
        }
    };
    Ok(name.to_string())
}

fn sort_mods(mods: &mut Vec<Modifier>) {
    // Canonical order so equality compares structurally regardless of
    // authoring order: Cmd, Ctrl, Alt, Shift.
    mods.sort_by_key(|m| match m {
        Modifier::Cmd => 0,
        Modifier::Ctrl => 1,
        Modifier::Alt => 2,
        Modifier::Shift => 3,
    });
}

fn format_keyref(k: &KeyRef) -> String {
    let mods = match k {
        KeyRef::Physical { mods, .. } | KeyRef::Character { mods, .. } => mods,
    };
    let body = match k {
        KeyRef::Physical { code, .. } => code.as_str(),
        KeyRef::Character { key, .. } => key.as_str(),
    };
    let mut parts: Vec<String> = mods
        .iter()
        .map(|m| match m {
            Modifier::Cmd => "Cmd".to_string(),
            Modifier::Ctrl => "Ctrl".to_string(),
            Modifier::Alt => "Alt".to_string(),
            Modifier::Shift => "Shift".to_string(),
        })
        .collect();
    parts.push(body.to_string());
    parts.join("+")
}

// ---------------------------------------------------------------------------
// location helpers (mirror layout.rs)
// ---------------------------------------------------------------------------

fn offset_to_line_col(src: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(src.len());
    let prefix = &src[..offset];
    let line = 1 + prefix.matches('\n').count();
    let column = match prefix.rfind('\n') {
        Some(nl) => prefix[nl + 1..].chars().count() + 1,
        None => prefix.chars().count() + 1,
    };
    (line as u32, column as u32)
}

fn node_loc(src: &str, node: &kdl::KdlNode) -> (u32, u32) {
    offset_to_line_col(src, node.span().offset())
}

fn entry_loc(src: &str, entry: &kdl::KdlEntry) -> (u32, u32) {
    offset_to_line_col(src, entry.span().offset())
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> ParsedKeymap {
        parse_keymap_kdl(src).expect("parse ok")
    }

    #[test]
    fn empty_document_parses_to_default() {
        let km = parse("");
        assert!(km.direct_binds.is_empty());
        assert!(km.trees.is_empty());
        assert!(km.prefixes.is_empty());
        assert_eq!(km.hud_default, HudMode::Always);
    }

    #[test]
    fn top_level_physical_bind() {
        let km = parse(r#"bind "Cmd+KeyK" "app.command-palette""#);
        assert_eq!(km.direct_binds.len(), 1);
        assert_eq!(
            km.direct_binds[0].key,
            KeyRef::Physical {
                mods: vec![Modifier::Cmd],
                code: "KeyK".into(),
            }
        );
        assert_eq!(
            km.direct_binds[0].action,
            KeymapAction::Command {
                id: "app.command-palette".into(),
            }
        );
    }

    #[test]
    fn alt_h_promotes_to_keyh() {
        let km = parse(r#"bind "Alt+h" "pane.focus-left""#);
        assert_eq!(
            km.direct_binds[0].key,
            KeyRef::Physical {
                mods: vec![Modifier::Alt],
                code: "KeyH".into(),
            }
        );
    }

    #[test]
    fn tmux_aliases_normalize() {
        let km = parse(r#"bind "C-b" "app.quit""#);
        assert_eq!(
            km.direct_binds[0].key,
            KeyRef::Physical {
                mods: vec![Modifier::Ctrl],
                code: "KeyB".into(),
            }
        );
        let km = parse(r#"bind "M-Left" "pane.focus-left""#);
        assert_eq!(
            km.direct_binds[0].key,
            KeyRef::Physical {
                mods: vec![Modifier::Alt],
                code: "ArrowLeft".into(),
            }
        );
    }

    #[test]
    fn tree_with_character_binds() {
        let km = parse(
            r#"
            tree "leader" {
                bind "h" "pane.focus-left"
                bind "%" "pane.split-vertical"
            }
            "#,
        );
        assert_eq!(km.trees.len(), 1);
        let tree = &km.trees[0];
        assert_eq!(tree.name, "leader");
        assert_eq!(
            tree.binds[0].key,
            KeyRef::Character {
                mods: vec![],
                key: "h".into(),
            }
        );
        assert_eq!(
            tree.binds[1].key,
            KeyRef::Character {
                mods: vec![],
                key: "%".into(),
            }
        );
    }

    #[test]
    fn tree_with_modifier_bind_uses_physical() {
        let km = parse(
            r#"
            tree "x" {
                bind "Ctrl+KeyC" "app.quit"
            }
            "#,
        );
        assert_eq!(
            km.trees[0].binds[0].key,
            KeyRef::Physical {
                mods: vec![Modifier::Ctrl],
                code: "KeyC".into(),
            }
        );
    }

    #[test]
    fn nested_enter_tree() {
        let km = parse(
            r#"
            tree "leader" {
                bind "w" { enter-tree "panes" }
            }
            "#,
        );
        assert_eq!(
            km.trees[0].binds[0].action,
            KeymapAction::EnterTree {
                tree: "panes".into(),
            }
        );
    }

    #[test]
    fn sticky_and_passthrough_as_child_nodes() {
        let km = parse(
            r#"
            tree "locked" {
                sticky
                passthrough
                bind "Escape" "keymap.exit-tree"
            }
            "#,
        );
        assert!(km.trees[0].sticky);
        assert!(km.trees[0].passthrough);
    }

    #[test]
    fn tree_hud_override_via_child_node() {
        let km = parse(
            r#"
            tree "tmux" {
                hud "delayed 1000"
                bind "h" "pane.focus-left"
            }
            "#,
        );
        assert_eq!(km.trees[0].hud, Some(HudMode::Delayed { ms: 1000 }));
    }

    #[test]
    fn prefix_declaration() {
        let km = parse(r#"prefix "Cmd+Semicolon" tree="leader""#);
        assert_eq!(km.prefixes.len(), 1);
        assert_eq!(km.prefixes[0].tree, "leader");
        assert_eq!(
            km.prefixes[0].key,
            KeyRef::Physical {
                mods: vec![Modifier::Cmd],
                code: "Semicolon".into(),
            }
        );
    }

    #[test]
    fn hud_modes() {
        assert_eq!(parse(r#"hud "always""#).hud_default, HudMode::Always);
        assert_eq!(parse(r#"hud "never""#).hud_default, HudMode::Never);
        assert_eq!(
            parse(r#"hud "delayed 1000""#).hud_default,
            HudMode::Delayed { ms: 1000 }
        );
    }

    #[test]
    fn preset_reference_captured() {
        let km = parse(
            r#"
            preset "default"
            bind "Cmd+KeyK" "app.command-palette"
            "#,
        );
        assert_eq!(km.preset_ref.as_deref(), Some("default"));
    }

    #[test]
    fn unbind_captured_for_merge() {
        let km = parse(r#"unbind "Alt+Digit0""#);
        assert_eq!(km.unbinds.len(), 1);
        assert!(matches!(
            km.unbinds[0],
            KeyRef::Physical { ref code, .. } if code == "Digit0"
        ));
    }

    #[test]
    fn duplicate_prefix_warns_first_wins() {
        let km = parse(
            r#"
            prefix "Ctrl+KeyA" tree="a"
            prefix "Ctrl+KeyA" tree="b"
            "#,
        );
        assert_eq!(km.prefixes.len(), 1);
        assert_eq!(km.prefixes[0].tree, "a");
        assert_eq!(km.warnings.len(), 1);
    }

    #[test]
    fn duplicate_direct_bind_warns_last_wins() {
        let km = parse(
            r#"
            bind "Cmd+KeyK" "app.command-palette"
            bind "Cmd+KeyK" "app.quit"
            "#,
        );
        assert_eq!(km.direct_binds.len(), 1);
        assert_eq!(
            km.direct_binds[0].action,
            KeymapAction::Command { id: "app.quit".into() }
        );
        assert_eq!(km.warnings.len(), 1);
    }

    #[test]
    fn prefix_shadows_direct_bind() {
        let km = parse(
            r#"
            bind "Ctrl+KeyB" "app.quit"
            prefix "Ctrl+KeyB" tree="tmux"
            "#,
        );
        assert_eq!(km.direct_binds.len(), 0);
        assert_eq!(km.prefixes.len(), 1);
        assert_eq!(km.warnings.len(), 1);
    }

    #[test]
    fn redeclaring_tree_replaces() {
        let km = parse(
            r#"
            tree "x" { bind "h" "pane.focus-left" }
            tree "x" { bind "j" "pane.focus-down" }
            "#,
        );
        assert_eq!(km.trees.len(), 1);
        assert_eq!(
            km.trees[0].binds[0].key,
            KeyRef::Character { mods: vec![], key: "j".into() }
        );
    }

    #[test]
    fn merge_overlay_replaces_bind() {
        let base = parse(r#"bind "Cmd+KeyK" "app.command-palette""#);
        let overlay = parse(r#"bind "Cmd+KeyK" "app.quit""#);
        let merged = merge_keymaps(base, overlay);
        assert_eq!(merged.direct_binds.len(), 1);
        assert_eq!(
            merged.direct_binds[0].action,
            KeymapAction::Command { id: "app.quit".into() }
        );
    }

    #[test]
    fn merge_unbind_removes_from_base() {
        let base = parse(r#"bind "Alt+Digit0" "pane.focus-index-10""#);
        let overlay = parse(r#"unbind "Alt+Digit0""#);
        let merged = merge_keymaps(base, overlay);
        assert!(merged.direct_binds.is_empty());
    }

    #[test]
    fn merge_replaces_tree_whole() {
        let base = parse(
            r#"
            tree "leader" {
                bind "h" "pane.focus-left"
                bind "j" "pane.focus-down"
            }
            "#,
        );
        let overlay = parse(r#"tree "leader" { bind "x" "pane.close" }"#);
        let merged = merge_keymaps(base, overlay);
        assert_eq!(merged.trees.len(), 1);
        assert_eq!(merged.trees[0].binds.len(), 1);
        assert_eq!(
            merged.trees[0].binds[0].key,
            KeyRef::Character { mods: vec![], key: "x".into() }
        );
    }

    #[test]
    fn unknown_top_level_node_errors() {
        let err = parse_keymap_kdl(r#"garbage "x""#).unwrap_err();
        matches!(err, KeymapParseError::Schema { .. });
    }

    #[test]
    fn bind_missing_args_errors() {
        assert!(parse_keymap_kdl(r#"bind "Cmd+KeyK""#).is_err());
    }

    #[test]
    fn parenthesized_word_without_enter_tree_errors() {
        let src = r#"
            tree "x" {
                bind "w" { some-other-node "foo" }
            }
        "#;
        assert!(parse_keymap_kdl(src).is_err());
    }

    #[test]
    fn syntax_error_reports_line_and_column() {
        let err = parse_keymap_kdl("bind \"Cmd+KeyK\" {}\ngarbage nonsense =\n").unwrap_err();
        match err {
            KeymapParseError::Syntax { line, .. } => {
                assert!(line >= 1);
            }
            _ => panic!("expected syntax error"),
        }
    }
}
