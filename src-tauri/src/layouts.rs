//! Layout loader: bundles built-in `.kdl` layouts via `include_str!` and reads
//! user-authored layouts from `~/.config/roux/layouts/*.kdl`.
//!
//! This module is the I/O boundary for the Phase 1 parser in
//! `roux_core::models::layout`. The parser is pure (string in, struct out);
//! everything filesystem-shaped lives here so the core crate stays
//! environment-free.
//!
//! Errors from a single bad user file do NOT poison the rest of the load —
//! the directory walker collects parse failures into `LoadedLayouts.errors`
//! and keeps going. Built-in failures are programming bugs (we shipped KDL
//! that doesn't parse) but we surface them the same way for symmetry; the
//! commands layer logs them and the binary still boots.

use std::path::{Path, PathBuf};

use roux_core::{parse_layout_kdl, LayoutSource, LayoutSpec};

/// Result of loading a set of layouts from disk (or `include_str!`). Holds
/// successful parses and parse failures separately so callers can show good
/// layouts even when some files are malformed.
#[derive(Debug, Default)]
pub struct LoadedLayouts {
    pub layouts: Vec<LayoutSpec>,
    pub errors: Vec<LayoutLoadError>,
}

/// A single layout file that failed to parse. `path` is the source file path
/// (or a synthetic path for built-ins) and `message` is the human-readable
/// error from the parser.
#[derive(Debug)]
pub struct LayoutLoadError {
    pub path: PathBuf,
    pub message: String,
}

/// Hard-coded list of built-in layouts. Each entry is `(stem, kdl_source)`.
/// The stem becomes the layout's `id`; the source is bundled into the binary
/// by `include_str!`. Adding a new built-in is a one-line edit here plus a
/// new `.kdl` file under `layouts/builtin/`.
const BUILTIN_LAYOUTS: &[(&str, &str)] = &[
    ("claude_shell", include_str!("layouts/builtin/claude_shell.kdl")),
    ("agent_comparison", include_str!("layouts/builtin/agent_comparison.kdl")),
];

/// Load all built-in layouts. The result is deterministic — same layouts in
/// the same order on every call. A parse failure here is a programming bug
/// (the source is checked into the repo and bundled at compile time), but we
/// still funnel it through `errors` rather than panicking so the app can
/// boot and the commands layer can log the diagnostic.
pub fn load_builtin_layouts() -> LoadedLayouts {
    let mut out = LoadedLayouts::default();
    for (stem, src) in BUILTIN_LAYOUTS {
        let synthetic_path = PathBuf::from(format!("<builtin>/{stem}.kdl"));
        match parse_layout_kdl(*stem, LayoutSource::Builtin, src) {
            Ok(spec) => out.layouts.push(spec),
            Err(e) => {
                out.errors.push(LayoutLoadError { path: synthetic_path, message: e.to_string() })
            }
        }
    }
    out
}

/// Load user-authored layouts from `~/.config/roux/layouts/*.kdl`. Creates
/// the directory if it doesn't exist (so first launch is silent rather than
/// crashing). See [`load_user_layouts_in`] for the testable inner.
pub fn load_user_layouts() -> LoadedLayouts {
    load_user_layouts_in(&crate::paths::roux_config_dir().join("layouts"))
}

/// Testable inner for [`load_user_layouts`]. Takes the directory path
/// explicitly so tests can point at a `tempfile::tempdir` without touching
/// the user's real config dir.
///
/// Behavior:
/// * Creates `dir` if missing. A failure to create the directory is logged
///   but otherwise yields an empty `LoadedLayouts` — we never want startup
///   to die because the layout dir is unwritable.
/// * Iterates only the top level. Subdirectories are NOT recursed into.
/// * Skips symlinks (using `symlink_metadata`, mirroring `paths::copy_dir_skip_existing`),
///   non-regular files, dotfiles, and anything whose extension isn't `.kdl`.
/// * Each remaining file is parsed; success → `layouts`, failure → `errors`.
///   The `id` for each layout is the filename stem, lowercased.
pub fn load_user_layouts_in(dir: &Path) -> LoadedLayouts {
    let mut out = LoadedLayouts::default();

    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            out.errors.push(LayoutLoadError {
                path: dir.to_path_buf(),
                message: format!("failed to create user layouts directory: {e}"),
            });
            return out;
        }
        // Freshly created → nothing to load.
        return out;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) => {
            out.errors.push(LayoutLoadError {
                path: dir.to_path_buf(),
                message: format!("failed to read user layouts directory: {e}"),
            });
            return out;
        }
    };

    // Collect entries first so we can sort them — read_dir order is
    // platform-defined, and a deterministic load order makes the dropdown
    // and tests stable.
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                out.errors.push(LayoutLoadError {
                    path: dir.to_path_buf(),
                    message: format!("failed to read directory entry: {e}"),
                });
                continue;
            }
        };
        paths.push(entry.path());
    }
    paths.sort();

    for path in paths {
        // `symlink_metadata` doesn't follow the final component, so a
        // symlink reports itself as a symlink instead of as whatever it
        // points at. Mirrors the guard in `paths::copy_dir_skip_existing`.
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                out.errors.push(LayoutLoadError {
                    path: path.clone(),
                    message: format!("failed to stat layout file: {e}"),
                });
                continue;
            }
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            continue;
        }
        if !ft.is_file() {
            // Sockets, fifos, devices — none of which we ever want to
            // attempt to read as KDL.
            continue;
        }

        // Skip dotfiles. `file_name()` is `Some` for any path returned by
        // read_dir, but we still defend against the impossible case.
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if file_name.starts_with('.') {
            continue;
        }

        // Only `.kdl` files. Extension match is case-insensitive only in
        // intent — we lowercase the comparison to keep `Layout.KDL` from
        // tripping up authors on case-preserving filesystems.
        let ext = path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase());
        if ext.as_deref() != Some("kdl") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_ascii_lowercase(),
            None => continue,
        };

        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                out.errors.push(LayoutLoadError {
                    path: path.clone(),
                    message: format!("failed to read file: {e}"),
                });
                continue;
            }
        };

        match parse_layout_kdl(&stem, LayoutSource::User, &src) {
            Ok(spec) => out.layouts.push(spec),
            Err(e) => {
                out.errors.push(LayoutLoadError { path: path.clone(), message: e.to_string() })
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write test file");
    }

    const VALID_KDL: &str = r#"layout {
    name "good"
    pane profile="x"
}
"#;

    #[test]
    fn builtin_layouts_parse() {
        let result = load_builtin_layouts();
        assert!(
            result.errors.is_empty(),
            "built-in layouts should parse cleanly; got errors: {:?}",
            result.errors
        );
        let ids: Vec<&str> = result.layouts.iter().map(|l| l.id.as_str()).collect();
        assert!(ids.contains(&"claude_shell"), "expected claude_shell in built-ins; got {ids:?}");
        assert!(
            ids.contains(&"agent_comparison"),
            "expected agent_comparison in built-ins; got {ids:?}"
        );
    }

    #[test]
    fn builtin_loader_tags_source_as_builtin() {
        let result = load_builtin_layouts();
        assert!(!result.layouts.is_empty());
        for layout in &result.layouts {
            assert_eq!(
                layout.source,
                LayoutSource::Builtin,
                "built-in layout {} should be tagged Builtin",
                layout.id
            );
        }
    }

    #[test]
    fn user_loader_separates_successes_from_errors() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("good.kdl"), VALID_KDL);
        // Malformed KDL: unterminated string and missing braces.
        write(&tmp.path().join("bad.kdl"), r#"layout { name "oops "#);

        let result = load_user_layouts_in(tmp.path());
        assert_eq!(
            result.layouts.len(),
            1,
            "expected exactly one good layout; got {:?}",
            result.layouts
        );
        assert_eq!(result.layouts[0].id, "good");
        assert_eq!(result.errors.len(), 1, "expected exactly one error; got {:?}", result.errors);
        let err_path = &result.errors[0].path;
        assert!(
            err_path.ends_with("bad.kdl"),
            "error path should end in bad.kdl; got {err_path:?}"
        );
    }

    #[test]
    fn user_loader_creates_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("does-not-exist-yet");
        assert!(!target.exists());

        let result = load_user_layouts_in(&target);
        assert!(result.layouts.is_empty());
        assert!(
            result.errors.is_empty(),
            "creating a fresh dir should not produce errors; got {:?}",
            result.errors
        );
        assert!(target.exists(), "directory should have been created");
        assert!(target.is_dir());
    }

    #[test]
    fn user_loader_ignores_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("good.kdl"), VALID_KDL);
        let nested = tmp.path().join("nested");
        fs::create_dir(&nested).unwrap();
        write(&nested.join("deep.kdl"), VALID_KDL);

        let result = load_user_layouts_in(tmp.path());
        assert!(result.errors.is_empty(), "got errors: {:?}", result.errors);
        let ids: Vec<&str> = result.layouts.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, vec!["good"], "nested file should be ignored; got {ids:?}");
    }

    #[test]
    #[cfg(unix)]
    fn user_loader_ignores_symlinks() {
        use std::os::unix::fs::symlink;

        // Two tempdirs: `tmp` is the layouts dir we load, `outside` holds
        // the file the symlink points at. Keeping the target out of `tmp`
        // ensures the only way the loader could see it is by following
        // the symlink.
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        write(&tmp.path().join("real.kdl"), VALID_KDL);
        let target = outside.path().join("linked-target.kdl");
        write(&target, VALID_KDL);
        // symlink(target, link) — argument order is opposite of `ln -s`.
        symlink(&target, tmp.path().join("linked.kdl")).expect("create symlink");

        let result = load_user_layouts_in(tmp.path());
        assert!(result.errors.is_empty(), "got errors: {:?}", result.errors);
        let ids: Vec<&str> = result.layouts.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, vec!["real"], "symlink should be skipped; got {ids:?}");
    }

    #[test]
    fn user_loader_derives_id_from_filename_stem() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("my_layout.kdl"),
            r#"layout {
    name "display name"
    pane profile="x"
}
"#,
        );

        let result = load_user_layouts_in(tmp.path());
        assert!(result.errors.is_empty(), "got errors: {:?}", result.errors);
        assert_eq!(result.layouts.len(), 1);
        assert_eq!(result.layouts[0].id, "my_layout");
        assert_eq!(result.layouts[0].name, "display name");
    }

    #[test]
    fn user_loader_ignores_non_kdl_files() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("good.kdl"), VALID_KDL);
        write(&tmp.path().join("README.md"), "not a layout");
        write(&tmp.path().join(".hidden"), "not a layout either");

        let result = load_user_layouts_in(tmp.path());
        assert!(result.errors.is_empty(), "got errors: {:?}", result.errors);
        let ids: Vec<&str> = result.layouts.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, vec!["good"], "only .kdl files should load; got {ids:?}");
    }

    #[test]
    fn user_loader_tags_source_as_user() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("u.kdl"), VALID_KDL);

        let result = load_user_layouts_in(tmp.path());
        assert_eq!(result.layouts.len(), 1);
        assert_eq!(result.layouts[0].source, LayoutSource::User);
    }
}
