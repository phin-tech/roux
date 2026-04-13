//! Tauri commands exposing the layout loader to the frontend.
//!
//! These are deliberately thin pass-throughs over `crate::layouts::*`. The
//! frontend (Phase 3+) calls them once on startup to populate the layout
//! dropdown in the New Session dialog. v1 logs parse failures to stderr but
//! does NOT surface them to the frontend — a `get_layout_load_diagnostics`
//! command can expose the warning list later when we build a settings panel.

use roux_core::LayoutSpec;

/// Return the bundled built-in layouts. Parse errors here would be a
/// programming bug (we shipped KDL that doesn't parse); they are logged to
/// stderr so they show up in the dev console but the command still returns
/// the successfully-parsed layouts.
#[tauri::command]
#[specta::specta]
pub(crate) fn get_builtin_layouts() -> Vec<LayoutSpec> {
    let result = crate::layouts::load_builtin_layouts();
    for err in &result.errors {
        eprintln!("roux: built-in layout failed to load {}: {}", err.path.display(), err.message);
    }
    result.layouts
}

/// Return user-authored layouts from `~/.config/roux/layouts/*.kdl`. Same
/// v1 policy as `get_builtin_layouts` — failures are logged, not surfaced.
#[tauri::command]
#[specta::specta]
pub(crate) fn get_user_layouts() -> Vec<LayoutSpec> {
    let result = crate::layouts::load_user_layouts();
    for err in &result.errors {
        eprintln!("roux: user layout failed to parse {}: {}", err.path.display(), err.message);
    }
    result.layouts
}
