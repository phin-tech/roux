//! Roux layouts v1 — types and KDL parser.
//!
//! A [`LayoutSpec`] describes the shape of a whole session: which panes
//! exist, how they split, and what [`super::profile::SpawnProfile`] runs in
//! each. Layouts are file-authored (`.kdl`), built-in or user, and drive the
//! "new session from layout" flow in the frontend.
//!
//! This module is the ONLY place in the codebase that knows about `kdl-rs`.
//! The public API exposes plain Rust types; callers above never see
//! `KdlDocument` / `KdlNode`. If you need to plumb more KDL surface into the
//! schema, add it here and keep the leakage boundary intact.

use serde::{Deserialize, Serialize};

use super::profile::{ProfileSource, Provider, SpawnProfile, StartupBehavior};

/// Direction of a split node in a layout. Matches `SplitDirection`
/// in `src/lib/panes/layout.ts` (`h`/`v`) but spelled out for KDL
/// authors who will reach for the Zellij spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LayoutSplitDirection {
    Horizontal,
    Vertical,
}

/// How a leaf pane references a spawn profile. Registered ids are looked up
/// at session-creation time against the merged profile registry; inline
/// profiles are authored directly in the layout file and carry their own
/// source = [`ProfileSource::Inline`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LayoutProfileRef {
    Registered { id: String },
    Inline { profile: SpawnProfile },
}

/// A node in a layout's pane tree. Leaves spawn a single shell seeded from a
/// profile; splits hold ordered children and a direction. The `size` field is
/// a raw proportional weight in `[0, 100]`; normalization to pane-tree
/// fractions happens later in the frontend walker, not here.
///
/// `Eq` is deliberately omitted: `size` is an `Option<f32>` and `f32` does
/// not implement `Eq`. Tests use `PartialEq` with exact float values, which
/// is fine because sizes come directly from literal tokens in the KDL source.
//
// The `Leaf` variant holds an inline `SpawnProfile` and is therefore larger
// than `Split`. We accept the size difference; boxing would add an
// allocation on every parse and ripple through serde/specta with no
// practical payoff — layouts are small, bounded trees.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LayoutPaneNode {
    Leaf {
        profile_ref: LayoutProfileRef,
        // Optional fields are serialized as `null` when unset rather than
        // omitted. specta's unified-mode type validator rejects
        // `skip_serializing_if` because it produces asymmetric types, and the
        // bytes saved by omission aren't worth forking serialize/deserialize.
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        size: Option<f32>,
        // Reserved; rejected at parse time in v1 so we don't ship a silently
        // ignored field. Kept on the struct so Phase 2 can light it up
        // without another migration.
        #[serde(default)]
        cwd: Option<String>,
        /// Optional nono sandbox profile name (e.g. "default", "permissive").
        #[serde(default)]
        nono_profile: Option<String>,
        /// Optional allow_dir entries from a `nono_flags` child block.
        #[serde(default)]
        nono_allow_dirs: Option<Vec<String>>,
    },
    Split {
        direction: LayoutSplitDirection,
        #[serde(default)]
        size: Option<f32>,
        children: Vec<LayoutPaneNode>,
    },
}

/// Where a layout came from. Built-in layouts are bundled with the app;
/// user layouts live under `<config>/roux/layouts/` (Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LayoutSource {
    Builtin,
    User,
}

/// A parsed layout. `id` is derived by the loader from the filename stem
/// (Phase 2) and passed in to [`parse_layout_kdl`] — it is intentionally not
/// read from the KDL source itself so renaming a file renames the layout.
///
/// `Eq` is omitted because the embedded [`LayoutPaneNode`] carries
/// `Option<f32>` sizes; see the note on [`LayoutPaneNode`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub source: LayoutSource,
    pub root: LayoutPaneNode,
}

/// Errors from [`parse_layout_kdl`]. Not a `specta::Type` — errors cross the
/// IPC boundary as `String`, matching every other service error in roux.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LayoutParseError {
    #[error("kdl parse error at {line}:{column}: {message}")]
    Syntax { line: usize, column: usize, message: String },
    #[error("{message} at {line}:{column}")]
    Schema { line: usize, column: usize, message: String },
}

impl LayoutParseError {
    fn schema(loc: (usize, usize), message: impl Into<String>) -> Self {
        Self::Schema { line: loc.0, column: loc.1, message: message.into() }
    }
}

/// Parse a KDL layout document into a [`LayoutSpec`].
///
/// `id` and `source` are provided by the caller (the loader, in Phase 2) —
/// they do not appear in the KDL source.
pub fn parse_layout_kdl(
    id: impl Into<String>,
    source: LayoutSource,
    src: &str,
) -> Result<LayoutSpec, LayoutParseError> {
    let doc: kdl::KdlDocument = src.parse().map_err(|e: kdl::KdlError| {
        // kdl-rs returns byte-offset spans; we only need the first diagnostic
        // for the top-level error message. Missing diagnostics is extremely
        // unlikely but we degrade gracefully to (0, 0) rather than panic.
        let (line, column, message) = e
            .diagnostics
            .first()
            .map(|d| {
                let (l, c) = offset_to_line_col(src, d.span.offset());
                (l, c, d.message.clone().unwrap_or_else(|| "invalid KDL syntax".to_string()))
            })
            .unwrap_or_else(|| (0, 0, "invalid KDL syntax".to_string()));
        LayoutParseError::Syntax { line, column, message }
    })?;

    let top_nodes: Vec<&kdl::KdlNode> = doc.nodes().iter().collect();
    let layout_node = match top_nodes.as_slice() {
        [single] if single.name().value() == "layout" => *single,
        [] => {
            return Err(LayoutParseError::schema(
                (1, 1),
                "expected a top-level `layout` node; document is empty",
            ));
        }
        [single] => {
            return Err(LayoutParseError::schema(
                node_loc(src, single),
                format!(
                    "unknown top-level node `{}`; expected a single `layout` node",
                    single.name().value()
                ),
            ));
        }
        [_, second, ..] => {
            return Err(LayoutParseError::schema(
                node_loc(src, second),
                "exactly one top-level `layout` node is permitted",
            ));
        }
    };

    parse_layout_node(id.into(), source, src, layout_node)
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

/// Convert a byte offset into `src` into a 1-indexed `(line, column)` pair.
///
/// kdl-rs reports spans as byte offsets, but our error type speaks
/// human-readable line/column. A layout file is small enough that scanning is
/// fine; we never need a persistent index.
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

/// Walk the `layout { ... }` node and produce a [`LayoutSpec`].
fn parse_layout_node(
    id: String,
    source: LayoutSource,
    src: &str,
    node: &kdl::KdlNode,
) -> Result<LayoutSpec, LayoutParseError> {
    // Reject attributes on the `layout` node — name/description are child
    // nodes in the schema, not attributes, and silently ignoring attributes
    // would make typos hard to find.
    if let Some(entry) = node.entries().iter().next() {
        return Err(LayoutParseError::schema(
            entry_loc(src, entry),
            "`layout` node takes no attributes; set `name` and `description` as child nodes",
        ));
    }

    let children = node.children().ok_or_else(|| {
        LayoutParseError::schema(node_loc(src, node), "`layout` must have a body `{ ... }`")
    })?;

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut root: Option<LayoutPaneNode> = None;

    for child in children.nodes() {
        match child.name().value() {
            "name" => {
                name = Some(single_string_arg(src, child, "name")?);
            }
            "description" => {
                description = Some(single_string_arg(src, child, "description")?);
            }
            "pane" => {
                if root.is_some() {
                    return Err(LayoutParseError::schema(
                        node_loc(src, child),
                        "`layout` must contain exactly one root `pane` node",
                    ));
                }
                root = Some(parse_pane_node(src, child)?);
            }
            other => {
                return Err(LayoutParseError::schema(
                    node_loc(src, child),
                    format!(
                        "unknown child node `{other}` in `layout`; expected `name`, `description`, or `pane`"
                    ),
                ));
            }
        }
    }

    let name = name.ok_or_else(|| {
        LayoutParseError::schema(
            node_loc(src, node),
            "`layout` is missing required `name` child node",
        )
    })?;
    let root = root.ok_or_else(|| {
        LayoutParseError::schema(
            node_loc(src, node),
            "`layout` is missing a root `pane` child node",
        )
    })?;

    Ok(LayoutSpec { id, name, description, source, root })
}

/// Expect a node with exactly one positional string argument and no
/// attributes; return the argument's value.
fn single_string_arg(
    src: &str,
    node: &kdl::KdlNode,
    what: &str,
) -> Result<String, LayoutParseError> {
    let args: Vec<&kdl::KdlEntry> = node.entries().iter().filter(|e| e.name().is_none()).collect();
    let attrs: Vec<&kdl::KdlEntry> = node.entries().iter().filter(|e| e.name().is_some()).collect();
    if !attrs.is_empty() {
        return Err(LayoutParseError::schema(
            entry_loc(src, attrs[0]),
            format!("`{what}` takes no attributes"),
        ));
    }
    let [arg] = args.as_slice() else {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            format!("`{what}` requires exactly one string argument"),
        ));
    };
    match arg.value() {
        kdl::KdlValue::String(s) => Ok(s.clone()),
        _ => Err(LayoutParseError::schema(
            entry_loc(src, arg),
            format!("`{what}` argument must be a string"),
        )),
    }
}

#[derive(Default)]
struct PaneAttrs {
    profile: Option<String>,
    split_direction: Option<String>,
    name: Option<String>,
    size: Option<f32>,
    // Captured here so we can reject with a precise error instead of the
    // generic "unknown attribute" message.
    has_cwd: bool,
    /// The `nono="profile_name"` attribute on a leaf pane.
    nono: Option<String>,
}

/// Parse a single `pane` node — either a leaf or a container.
fn parse_pane_node(src: &str, node: &kdl::KdlNode) -> Result<LayoutPaneNode, LayoutParseError> {
    let mut attrs = PaneAttrs::default();

    for entry in node.entries() {
        let Some(attr_name) = entry.name().map(|i| i.value()) else {
            return Err(LayoutParseError::schema(
                entry_loc(src, entry),
                "`pane` does not take positional arguments; use `key=value` attributes",
            ));
        };
        match attr_name {
            "profile" => {
                attrs.profile = Some(string_value(src, entry, "profile")?);
            }
            "split_direction" => {
                attrs.split_direction = Some(string_value(src, entry, "split_direction")?);
            }
            "name" => {
                attrs.name = Some(string_value(src, entry, "name")?);
            }
            "size" => {
                let n = number_value(src, entry, "size")?;
                if !(0.0..=100.0).contains(&n) {
                    return Err(LayoutParseError::schema(
                        entry_loc(src, entry),
                        format!("`size` must be in range [0, 100]; got {n}"),
                    ));
                }
                attrs.size = Some(n);
            }
            "cwd" => {
                attrs.has_cwd = true;
            }
            "nono" => {
                attrs.nono = Some(string_value(src, entry, "nono")?);
            }
            other => {
                return Err(LayoutParseError::schema(
                    entry_loc(src, entry),
                    format!(
                        "unknown `pane` attribute `{other}`; valid: profile, split_direction, name, size, nono"
                    ),
                ));
            }
        }
    }

    if attrs.has_cwd {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            "`cwd` on pane is not supported in v1; remove it",
        ));
    }

    let has_children = node.children().map(|d| !d.nodes().is_empty()).unwrap_or(false);

    // Decision tree: split vs leaf vs inline-profile leaf.
    //
    // A pane is a SPLIT iff it has `split_direction`. Splits require
    // children and no leaf attributes.
    //
    // A pane is a LEAF otherwise. A leaf either references a registered
    // profile (`profile="id"`) OR defines an inline profile via a body block,
    // but not both. An inline-profile leaf still has a `{ ... }` body, so
    // "has children" alone doesn't distinguish — we check `split_direction`
    // first.
    if let Some(dir_str) = attrs.split_direction.as_deref() {
        return parse_split_pane(src, node, dir_str, &attrs, has_children);
    }

    parse_leaf_pane(src, node, &attrs, has_children)
}

fn parse_split_pane(
    src: &str,
    node: &kdl::KdlNode,
    dir_str: &str,
    attrs: &PaneAttrs,
    has_children: bool,
) -> Result<LayoutPaneNode, LayoutParseError> {
    let direction = match dir_str {
        "horizontal" => LayoutSplitDirection::Horizontal,
        "vertical" => LayoutSplitDirection::Vertical,
        other => {
            return Err(LayoutParseError::schema(
                node_loc(src, node),
                format!("invalid `split_direction` `{other}`; valid values: horizontal, vertical"),
            ));
        }
    };

    if attrs.profile.is_some() {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            "a split pane cannot also set `profile`",
        ));
    }
    if attrs.name.is_some() {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            "`name` is only valid on leaf panes",
        ));
    }
    if attrs.nono.is_some() {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            "`nono` is only valid on leaf panes; a split pane applies no sandboxing of its own",
        ));
    }
    if !has_children {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            "split pane requires at least one child `pane`",
        ));
    }

    let kdl_children = node.children().expect("has_children verified above");
    let mut children = Vec::new();
    for child in kdl_children.nodes() {
        let cname = child.name().value();
        if cname == "nono_flags" {
            return Err(LayoutParseError::schema(
                node_loc(src, child),
                "`nono_flags` is only valid inside a leaf pane; a split pane does not accept sandbox flags",
            ));
        }
        if cname != "pane" {
            return Err(LayoutParseError::schema(
                node_loc(src, child),
                format!("unexpected child `{cname}` inside split pane; only `pane` is allowed"),
            ));
        }
        children.push(parse_pane_node(src, child)?);
    }

    Ok(LayoutPaneNode::Split { direction, size: attrs.size, children })
}

fn parse_leaf_pane(
    src: &str,
    node: &kdl::KdlNode,
    attrs: &PaneAttrs,
    has_children: bool,
) -> Result<LayoutPaneNode, LayoutParseError> {
    // Body disambiguation: a pane's child block can contain inline-profile
    // fields, `nono_flags` metadata, or both (when no registered profile is
    // set). When `profile="id"` IS set, only `nono_flags` nodes are
    // permitted — any inline-profile field is a mutual-exclusivity error.
    let mut nono_allow_dirs: Option<Vec<String>> = None;

    let profile_ref = match (&attrs.profile, has_children) {
        (Some(id), true) => {
            // Registered profile + body: only `nono_flags` children allowed.
            let body = node.children().expect("has_children is true");
            let has_inline_fields = body.nodes().iter().any(|c| c.name().value() != "nono_flags");
            if has_inline_fields {
                return Err(LayoutParseError::schema(
                    node_loc(src, node),
                    "leaf pane cannot set both `profile=\"id\"` and an inline profile body (they are mutually exclusive)",
                ));
            }
            // Parse nono_flags if present.
            nono_allow_dirs = parse_nono_flags_from_children(src, body)?;
            LayoutProfileRef::Registered { id: id.clone() }
        }
        (Some(id), false) => LayoutProfileRef::Registered { id: id.clone() },
        (None, true) => {
            let body = node.children().expect("has_children is true");
            // Parse nono_flags before handing body to inline profile parser
            // (which will skip nono_flags nodes).
            nono_allow_dirs = parse_nono_flags_from_children(src, body)?;
            let profile = parse_inline_profile(src, node, attrs.name.as_deref(), body)?;
            LayoutProfileRef::Inline { profile }
        }
        (None, false) => {
            // Ambiguous: no profile, no body. Could be a forgotten
            // `split_direction` container (if the author meant to put
            // children inside), or a truly empty leaf. Nudge them toward the
            // likely fix.
            return Err(LayoutParseError::schema(
                node_loc(src, node),
                "`pane` needs either `profile=\"id\"`, an inline profile body, or `split_direction=...` with children",
            ));
        }
    };

    // Validate: nono_flags without nono attribute is meaningless.
    if nono_allow_dirs.is_some() && attrs.nono.is_none() {
        return Err(LayoutParseError::schema(
            node_loc(src, node),
            "`nono_flags` requires a `nono=\"...\"` attribute on the pane",
        ));
    }

    Ok(LayoutPaneNode::Leaf {
        profile_ref,
        name: attrs.name.clone(),
        size: attrs.size,
        cwd: None,
        nono_profile: attrs.nono.clone(),
        nono_allow_dirs,
    })
}

/// Scan a pane's child document for `nono_flags` nodes and parse `allow_dir`
/// entries. Returns `None` if no `nono_flags` block is present; returns
/// `Some(dirs)` if one is found. Multiple `nono_flags` blocks are an error.
fn parse_nono_flags_from_children(
    src: &str,
    body: &kdl::KdlDocument,
) -> Result<Option<Vec<String>>, LayoutParseError> {
    let nono_nodes: Vec<&kdl::KdlNode> =
        body.nodes().iter().filter(|n| n.name().value() == "nono_flags").collect();

    let nono_node = match nono_nodes.as_slice() {
        [] => return Ok(None),
        [single] => *single,
        [_, dup, ..] => {
            return Err(LayoutParseError::schema(
                node_loc(src, dup),
                "only one `nono_flags` block is allowed per pane",
            ));
        }
    };

    // nono_flags must not have attributes
    if let Some(entry) = nono_node.entries().iter().next() {
        return Err(LayoutParseError::schema(
            entry_loc(src, entry),
            "`nono_flags` takes no attributes",
        ));
    }

    let flags_body = nono_node.children().ok_or_else(|| {
        LayoutParseError::schema(node_loc(src, nono_node), "`nono_flags` requires a body `{ ... }`")
    })?;

    let mut dirs = Vec::new();
    for child in flags_body.nodes() {
        match child.name().value() {
            "allow_dir" => {
                dirs.push(single_string_arg(src, child, "allow_dir")?);
            }
            other => {
                return Err(LayoutParseError::schema(
                    node_loc(src, child),
                    format!("unknown `nono_flags` child `{other}`; valid: allow_dir"),
                ));
            }
        }
    }

    Ok(Some(dirs))
}

fn string_value(src: &str, entry: &kdl::KdlEntry, what: &str) -> Result<String, LayoutParseError> {
    match entry.value() {
        kdl::KdlValue::String(s) => Ok(s.clone()),
        _ => Err(LayoutParseError::schema(
            entry_loc(src, entry),
            format!("`{what}` must be a string"),
        )),
    }
}

fn number_value(src: &str, entry: &kdl::KdlEntry, what: &str) -> Result<f32, LayoutParseError> {
    match entry.value() {
        kdl::KdlValue::Integer(i) => Ok(*i as f32),
        kdl::KdlValue::Float(f) => Ok(*f as f32),
        _ => Err(LayoutParseError::schema(
            entry_loc(src, entry),
            format!("`{what}` must be a number"),
        )),
    }
}

/// Parse an inline profile block on a leaf pane into a [`SpawnProfile`] with
/// `source = Inline`. Called only when the leaf has no `profile="id"` attr.
fn parse_inline_profile(
    src: &str,
    pane_node: &kdl::KdlNode,
    pane_name: Option<&str>,
    body: &kdl::KdlDocument,
) -> Result<SpawnProfile, LayoutParseError> {
    let mut display_name: Option<String> = None;
    let mut kind: Option<Provider> = None; // None = no provider = shell
    let mut kind_seen = false;
    let mut setup_command: Option<String> = None;
    let mut startup_command: Option<String> = None;
    let mut startup_behavior: Option<StartupBehavior> = None;
    let mut env: Option<std::collections::BTreeMap<String, String>> = None;

    for child in body.nodes() {
        match child.name().value() {
            "display_name" => {
                display_name = Some(single_string_arg(src, child, "display_name")?);
            }
            "kind" => {
                let v = single_string_arg(src, child, "kind")?;
                kind_seen = true;
                kind = match v.as_str() {
                    "shell" => None,
                    "claude" => Some(Provider::Claude),
                    "codex" => Some(Provider::Codex),
                    other => {
                        return Err(LayoutParseError::schema(
                            node_loc(src, child),
                            format!(
                                "invalid inline profile `kind` `{other}`; valid values: shell, claude, codex"
                            ),
                        ));
                    }
                };
            }
            "setup_command" => {
                setup_command = Some(single_string_arg(src, child, "setup_command")?);
            }
            "startup_command" => {
                startup_command = Some(single_string_arg(src, child, "startup_command")?);
            }
            "startup_behavior" => {
                let v = single_string_arg(src, child, "startup_behavior")?;
                startup_behavior = Some(match v.as_str() {
                    // "run" is the alias used in the schema example; treat
                    // it as sugar for auto_run since that is the default
                    // and the obvious reading of "just run it".
                    "auto_run" | "run" => StartupBehavior::AutoRun,
                    "type_only" => StartupBehavior::TypeOnly,
                    other => {
                        return Err(LayoutParseError::schema(
                            node_loc(src, child),
                            format!(
                                "invalid `startup_behavior` `{other}`; valid values: auto_run, type_only"
                            ),
                        ));
                    }
                });
            }
            "env" => {
                let env_body = child.children().ok_or_else(|| {
                    LayoutParseError::schema(
                        node_loc(src, child),
                        "`env` requires a body `{ KEY \"value\"; ... }`",
                    )
                })?;
                let mut map = std::collections::BTreeMap::new();
                for kv in env_body.nodes() {
                    let key = kv.name().value().to_string();
                    let entries: Vec<&kdl::KdlEntry> = kv.entries().iter().collect();
                    let [entry] = entries.as_slice() else {
                        return Err(LayoutParseError::schema(
                            node_loc(src, kv),
                            format!("`env` entry `{key}` must have exactly one string value"),
                        ));
                    };
                    let value = match entry.value() {
                        kdl::KdlValue::String(s) => s.clone(),
                        _ => {
                            return Err(LayoutParseError::schema(
                                entry_loc(src, entry),
                                format!("`env` entry `{key}` value must be a string"),
                            ));
                        }
                    };
                    map.insert(key, value);
                }
                env = Some(map);
            }
            // nono_flags is parsed by the caller (parse_leaf_pane); skip it here.
            "nono_flags" => {}
            other => {
                return Err(LayoutParseError::schema(
                    node_loc(src, child),
                    format!(
                        "unknown inline profile field `{other}`; valid: display_name, kind, setup_command, startup_command, startup_behavior, env"
                    ),
                ));
            }
        }
    }

    // An inline profile with no `kind` declaration defaults to shell. This
    // is the most common case ("just run this command") and matches user
    // intuition. We still record whether kind was explicitly seen so we can
    // emit a more helpful error later if we ever add required-kind modes.
    let _ = kind_seen;

    // The layout author can give the pane a `name="..."` attribute; if so,
    // that's the profile's display name. Falls back to an explicit
    // `display_name` child, then to a generic label anchored to the pane
    // node's location.
    let name = display_name.or_else(|| pane_name.map(|s| s.to_string())).unwrap_or_else(|| {
        let (line, _) = node_loc(src, pane_node);
        format!("inline pane at line {line}")
    });

    Ok(SpawnProfile {
        // Inline profiles have no registry id. We mint a synthetic one so
        // any downstream code that expects `id` to be non-empty doesn't
        // break; it is not meant to be user-visible.
        id: format!("inline:{}", sanitize_for_id(&name)),
        name,
        setup_command,
        startup_command,
        startup_behavior,
        env,
        cwd_override: None,
        icon: None,
        provider: kind,
        nono_profile: None,
        nono_allow_dirs: None,
        source: ProfileSource::Inline,
    })
}

fn sanitize_for_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<LayoutSpec, LayoutParseError> {
        parse_layout_kdl("test-id", LayoutSource::User, src)
    }

    fn expect_schema_err(res: Result<LayoutSpec, LayoutParseError>) -> (usize, usize, String) {
        match res {
            Err(LayoutParseError::Schema { line, column, message }) => (line, column, message),
            Err(other) => panic!("expected Schema error, got {other:?}"),
            Ok(spec) => panic!("expected Schema error, got Ok({spec:?})"),
        }
    }

    #[test]
    fn parses_minimal_single_leaf() {
        let src = r#"layout {
            name "solo"
            pane profile="claude"
        }"#;
        let spec = parse(src).unwrap();
        assert_eq!(spec.id, "test-id");
        assert_eq!(spec.name, "solo");
        assert_eq!(spec.description, None);
        assert_eq!(spec.source, LayoutSource::User);
        match spec.root {
            LayoutPaneNode::Leaf {
                profile_ref,
                name,
                size,
                cwd,
                nono_profile,
                nono_allow_dirs,
            } => {
                assert_eq!(profile_ref, LayoutProfileRef::Registered { id: "claude".into() });
                assert_eq!(name, None);
                assert_eq!(size, None);
                assert_eq!(cwd, None);
                assert_eq!(nono_profile, None);
                assert_eq!(nono_allow_dirs, None);
            }
            other => panic!("expected Leaf, got {other:?}"),
        }
    }

    #[test]
    fn parses_horizontal_split() {
        let src = r#"layout {
            name "two-up"
            pane split_direction="horizontal" {
                pane profile="left"
                pane profile="right"
            }
        }"#;
        let spec = parse(src).unwrap();
        match spec.root {
            LayoutPaneNode::Split { direction, children, size } => {
                assert_eq!(direction, LayoutSplitDirection::Horizontal);
                assert_eq!(size, None);
                assert_eq!(children.len(), 2);
                for (child, expected_id) in children.iter().zip(["left", "right"]) {
                    match child {
                        LayoutPaneNode::Leaf { profile_ref, .. } => {
                            assert_eq!(
                                profile_ref,
                                &LayoutProfileRef::Registered { id: expected_id.into() }
                            );
                        }
                        other => panic!("expected Leaf, got {other:?}"),
                    }
                }
            }
            other => panic!("expected Split, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_split() {
        let src = r#"layout {
            name "2x2"
            pane split_direction="horizontal" {
                pane profile="a" size=50
                pane split_direction="vertical" size=50 {
                    pane profile="b" size=60
                    pane profile="c" size=40
                }
            }
        }"#;
        let spec = parse(src).unwrap();
        let LayoutPaneNode::Split { direction, children, .. } = spec.root else {
            panic!("expected outer Split");
        };
        assert_eq!(direction, LayoutSplitDirection::Horizontal);
        assert_eq!(children.len(), 2);
        match &children[0] {
            LayoutPaneNode::Leaf { profile_ref, size, .. } => {
                assert_eq!(profile_ref, &LayoutProfileRef::Registered { id: "a".into() });
                assert_eq!(*size, Some(50.0));
            }
            other => panic!("expected first Leaf, got {other:?}"),
        }
        match &children[1] {
            LayoutPaneNode::Split { direction, children: inner, size } => {
                assert_eq!(*direction, LayoutSplitDirection::Vertical);
                assert_eq!(*size, Some(50.0));
                assert_eq!(inner.len(), 2);
                match &inner[0] {
                    LayoutPaneNode::Leaf { profile_ref, size, .. } => {
                        assert_eq!(profile_ref, &LayoutProfileRef::Registered { id: "b".into() });
                        assert_eq!(*size, Some(60.0));
                    }
                    other => panic!("expected inner Leaf b, got {other:?}"),
                }
                match &inner[1] {
                    LayoutPaneNode::Leaf { profile_ref, size, .. } => {
                        assert_eq!(profile_ref, &LayoutProfileRef::Registered { id: "c".into() });
                        assert_eq!(*size, Some(40.0));
                    }
                    other => panic!("expected inner Leaf c, got {other:?}"),
                }
            }
            other => panic!("expected nested Split, got {other:?}"),
        }
    }

    #[test]
    fn parses_inline_profile_block() {
        let src = r#"layout {
            name "inline"
            pane name="tests" {
                kind "shell"
                startup_command "pytest"
            }
        }"#;
        let spec = parse(src).unwrap();
        match spec.root {
            LayoutPaneNode::Leaf { profile_ref, name, .. } => {
                assert_eq!(name, Some("tests".into()));
                match profile_ref {
                    LayoutProfileRef::Inline { profile } => {
                        assert_eq!(profile.source, ProfileSource::Inline);
                        assert_eq!(profile.provider, None);
                        assert_eq!(profile.startup_command, Some("pytest".into()));
                        // The leaf's `name="tests"` becomes the profile's
                        // display name when no explicit display_name is set.
                        assert_eq!(profile.name, "tests");
                    }
                    other => panic!("expected Inline, got {other:?}"),
                }
            }
            other => panic!("expected Leaf, got {other:?}"),
        }
    }

    #[test]
    fn rejects_profile_and_inline_block() {
        let src = r#"layout {
            name "both"
            pane profile="registered" {
                kind "shell"
                startup_command "oops"
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0, "want line/column, got {line}:{column}");
        assert!(
            message.contains("mutually exclusive") || message.contains("cannot set both"),
            "message should mention mutual exclusion; got: {message}"
        );
    }

    #[test]
    fn rejects_container_missing_split_direction() {
        // A pane with no profile and no split_direction, but with only
        // non-profile children (actual pane nodes), reads as a container that
        // forgot its direction.
        let src = r#"layout {
            name "bad"
            pane {
                pane profile="a"
                pane profile="b"
            }
        }"#;
        let err = parse(src);
        // This particular case ends up as an inline-profile leaf whose body
        // has unknown fields `pane`. Either framing of the error is
        // acceptable as long as the author learns their body is wrong.
        let (line, column, message) = expect_schema_err(err);
        assert!(line > 0 && column > 0);
        assert!(
            message.contains("pane")
                || message.contains("split_direction")
                || message.contains("unknown inline profile field"),
            "message should point at the container-vs-inline confusion; got: {message}"
        );
    }

    #[test]
    fn rejects_unknown_top_level_node() {
        let src = r#"workspace {
            name "nope"
            pane profile="x"
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.contains("workspace") || message.contains("layout"),
            "message should mention the offending or expected name; got: {message}"
        );
    }

    #[test]
    fn rejects_diagonal_split() {
        let src = r#"layout {
            name "diag"
            pane split_direction="diagonal" {
                pane profile="a"
                pane profile="b"
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(message.contains("horizontal"), "should list valid values: {message}");
        assert!(message.contains("vertical"), "should list valid values: {message}");
    }

    #[test]
    fn rejects_nono_on_split_pane() {
        // nono only affects PTY spawn, which happens on leaves. Accepting
        // it on a split would silently drop the sandbox, which is worse
        // than rejecting at parse time.
        let src = r#"layout {
            name "bad"
            pane split_direction="horizontal" nono="default" {
                pane profile="claude"
                pane profile="plain-shell"
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.to_lowercase().contains("nono"),
            "message should mention nono; got: {message}"
        );
    }

    #[test]
    fn rejects_nono_flags_on_split_pane() {
        // Same rationale: flags are meaningless without a leaf PTY.
        let src = r#"layout {
            name "bad"
            pane split_direction="horizontal" {
                nono_flags {
                    allow_dir "/tmp"
                }
                pane profile="claude"
                pane profile="plain-shell"
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.to_lowercase().contains("nono"),
            "message should mention nono_flags; got: {message}"
        );
    }

    #[test]
    fn rejects_out_of_range_size_negative() {
        let src = r#"layout {
            name "neg"
            pane split_direction="horizontal" {
                pane profile="a" size=-5
                pane profile="b"
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.contains("range") || message.contains("[0, 100]"),
            "should mention range: {message}"
        );
    }

    #[test]
    fn rejects_out_of_range_size_too_big() {
        let src = r#"layout {
            name "big"
            pane split_direction="horizontal" {
                pane profile="a" size=150
                pane profile="b"
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.contains("range") || message.contains("[0, 100]"),
            "should mention range: {message}"
        );
    }

    #[test]
    fn rejects_non_string_env_value() {
        let src = r#"layout {
            name "envbad"
            pane name="tests" {
                kind "shell"
                startup_command "pytest"
                env {
                    PORT 42
                }
            }
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.contains("env") && message.contains("string"),
            "should mention env/string: {message}"
        );
    }

    #[test]
    fn rejects_layout_missing_name() {
        let src = r#"layout {
            pane profile="x"
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(message.contains("name"), "should mention missing name: {message}");
    }

    #[test]
    fn rejects_two_layout_blocks() {
        let src = r#"layout {
            name "one"
            pane profile="a"
        }
        layout {
            name "two"
            pane profile="b"
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(
            message.contains("exactly one") || message.contains("one top-level"),
            "should mention single-layout rule: {message}"
        );
    }

    #[test]
    fn rejects_cwd_on_leaf() {
        let src = r#"layout {
            name "cwd"
            pane profile="x" cwd="/tmp"
        }"#;
        let (line, column, message) = expect_schema_err(parse(src));
        assert!(line > 0 && column > 0);
        assert!(message.contains("cwd"), "should mention cwd: {message}");
    }

    #[test]
    fn maps_startup_behavior_type_only() {
        let src = r#"layout {
            name "type only"
            pane name="t" {
                kind "claude"
                startup_command "claude"
                startup_behavior "type_only"
            }
        }"#;
        let spec = parse(src).unwrap();
        let LayoutPaneNode::Leaf { profile_ref, .. } = spec.root else {
            panic!("expected Leaf");
        };
        let LayoutProfileRef::Inline { profile } = profile_ref else {
            panic!("expected Inline profile");
        };
        assert_eq!(profile.startup_behavior, Some(StartupBehavior::TypeOnly));
        assert_eq!(profile.provider, Some(Provider::Claude));
    }

    #[test]
    fn parses_degenerate_single_leaf_no_split() {
        // A layout whose root is a bare leaf, with no outer split wrapper,
        // is valid. The walker that consumes LayoutSpec treats a lone Leaf
        // the same as a one-child split, so we don't require authors to add
        // a pointless wrapper.
        let src = r#"layout {
            name "x"
            pane profile="x"
        }"#;
        let spec = parse(src).unwrap();
        assert!(matches!(spec.root, LayoutPaneNode::Leaf { .. }));
    }

    // -----------------------------------------------------------------------
    // nono attribute and nono_flags tests
    // -----------------------------------------------------------------------

    #[test]
    fn parses_nono_attribute_on_leaf() {
        let src = r#"layout { name "test"; pane profile="claude" nono="default" }"#;
        let got = parse(src).unwrap();
        match &got.root {
            LayoutPaneNode::Leaf { nono_profile, nono_allow_dirs, .. } => {
                assert_eq!(nono_profile.as_deref(), Some("default"));
                assert!(nono_allow_dirs.is_none());
            }
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn parses_nono_flags_with_registered_profile() {
        let src = r#"
            layout {
                name "test"
                pane profile="claude" nono="permissive" {
                    nono_flags {
                        allow_dir "/tmp/scratch"
                        allow_dir "/opt/tools"
                    }
                }
            }
        "#;
        let got = parse(src).unwrap();
        match &got.root {
            LayoutPaneNode::Leaf { nono_profile, nono_allow_dirs, profile_ref, .. } => {
                assert_eq!(nono_profile.as_deref(), Some("permissive"));
                assert!(matches!(profile_ref, LayoutProfileRef::Registered { .. }));
                let dirs = nono_allow_dirs.as_ref().unwrap();
                assert_eq!(dirs.len(), 2);
                assert!(dirs.contains(&"/tmp/scratch".to_string()));
                assert!(dirs.contains(&"/opt/tools".to_string()));
            }
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn parses_nono_with_inline_profile() {
        let src = r#"
            layout {
                name "test"
                pane nono="default" {
                    kind "shell"
                    startup_command "my-agent"
                    nono_flags {
                        allow_dir "/opt"
                    }
                }
            }
        "#;
        let got = parse(src).unwrap();
        match &got.root {
            LayoutPaneNode::Leaf { nono_profile, nono_allow_dirs, profile_ref, .. } => {
                assert_eq!(nono_profile.as_deref(), Some("default"));
                assert!(matches!(profile_ref, LayoutProfileRef::Inline { .. }));
                assert_eq!(nono_allow_dirs.as_ref().unwrap().len(), 1);
            }
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn rejects_nono_flags_without_nono_attribute() {
        let src = r#"
            layout {
                name "test"
                pane profile="claude" {
                    nono_flags { allow_dir "/tmp" }
                }
            }
        "#;
        let err = parse(src).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("nono"));
    }

    #[test]
    fn rejects_registered_profile_with_inline_fields_and_nono_flags() {
        let src = r#"
            layout {
                name "test"
                pane profile="claude" nono="default" {
                    startup_command "echo hi"
                    nono_flags { allow_dir "/tmp" }
                }
            }
        "#;
        let err = parse(src).unwrap_err();
        // Should reject because you can't mix inline profile fields with profile="id"
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("mutually exclusive") || msg.contains("cannot") || msg.contains("profile")
        );
    }

    #[test]
    fn leaf_without_nono_has_none_fields() {
        let src = r#"layout { name "test"; pane profile="claude" }"#;
        let got = parse(src).unwrap();
        match &got.root {
            LayoutPaneNode::Leaf { nono_profile, nono_allow_dirs, .. } => {
                assert!(nono_profile.is_none());
                assert!(nono_allow_dirs.is_none());
            }
            _ => panic!("expected leaf"),
        }
    }
}
