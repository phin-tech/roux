//! Integration tests that exercise `roux-worktrunk` against a real `wt`
//! binary on PATH and real temp git repositories.
//!
//! Gated behind the `real-wt-integration` feature so the default
//! `cargo test --workspace` does not require `wt` installed. CI runs
//! `cargo test -p roux-worktrunk --features real-wt-integration`.
//!
//! These tests are the primary source of confidence that Roux's
//! integration layer reflects reality — not parser theatrics.

#![cfg(feature = "real-wt-integration")]

use std::path::{Path, PathBuf};
use std::process::Command;

use roux_worktrunk::{
    create_worktree, detect_wt, detect_wt_config, extract_hook_defs, list_logs, list_worktrees,
    remove_worktree, show_config, CreateOpts, RemoveOpts, WtBinary, WtError, WtItem,
    MIN_WT_VERSION,
};
use semver::Version;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Test harness (mirrors the tiny git helpers from roux-core/src/worktree.rs).
// Copied rather than extracted: two call sites do not yet earn an abstraction.
// ---------------------------------------------------------------------------

/// Spawn `git` with the user's global/system config ignored so tests don't
/// inherit commit-signing, credential helpers, or other host-specific
/// settings that flake under parallel load (e.g. 1Password SSH signing).
fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("failed to invoke git");
    assert!(
        out.status.success(),
        "git {:?} in {} failed: {}",
        args,
        repo.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("mkdir repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.test"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    (tmp, repo)
}

fn wt() -> WtBinary {
    detect_wt(None).expect(
        "wt binary must be on PATH for real-wt-integration tests \
         (install via `cargo install worktrunk`)",
    )
}

// ---------------------------------------------------------------------------
// detect_wt
// ---------------------------------------------------------------------------

#[test]
fn detect_wt_resolves_local_binary() {
    let w = wt();
    assert!(w.path.exists(), "resolved wt path must exist: {:?}", w.path);
    let floor = Version::parse(MIN_WT_VERSION).unwrap();
    assert!(
        w.version >= floor,
        "wt {} is below the declared floor {} — install a newer wt",
        w.version,
        floor
    );
}

#[test]
fn detect_wt_honors_settings_override() {
    let real = wt();
    let override_path = real.path.to_string_lossy().into_owned();
    let via_override =
        detect_wt(Some(&override_path)).expect("override path should resolve the same binary");
    assert_eq!(via_override.path, real.path);
    assert_eq!(via_override.version, real.version);
}

#[test]
fn detect_wt_returns_none_for_missing_override_path() {
    assert!(detect_wt(Some("/tmp/definitely/does/not/exist/wt")).is_none());
}

#[test]
fn detect_wt_returns_none_for_empty_override_falls_back_to_path() {
    // Empty string should NOT be treated as an explicit missing-path;
    // it falls through to `which("wt")` on PATH. We assume CI / dev
    // has wt installed here.
    let via_empty = detect_wt(Some(""));
    assert!(via_empty.is_some(), "empty override should fall through to PATH lookup");
}

// ---------------------------------------------------------------------------
// detect_wt_config
// ---------------------------------------------------------------------------

#[test]
fn detect_wt_config_true_when_config_toml_present() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".config")).unwrap();
    std::fs::write(repo.join(".config").join("wt.toml"), "# empty\n").unwrap();
    assert!(detect_wt_config(repo));
}

#[test]
fn detect_wt_config_false_when_no_config_dir() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!detect_wt_config(tmp.path()));
}

#[test]
fn detect_wt_config_false_when_config_dir_but_no_toml() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".config")).unwrap();
    assert!(!detect_wt_config(tmp.path()));
}

#[test]
fn detect_wt_config_false_when_path_is_directory_named_wt_toml() {
    // .config/wt.toml must be a file, not a directory.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".config").join("wt.toml")).unwrap();
    assert!(!detect_wt_config(tmp.path()));
}

// ---------------------------------------------------------------------------
// list_worktrees — real git + real wt
// ---------------------------------------------------------------------------

#[test]
fn list_worktrees_fresh_repo_has_single_main_entry() {
    let (_tmp, repo) = init_repo();
    let items = list_worktrees(&wt(), &repo).expect("list should succeed");
    assert_eq!(items.len(), 1, "fresh repo should have exactly one worktree");
    let main = &items[0];
    assert!(main.is_main, "only entry should be is_main=true");
    assert_eq!(main.branch.as_deref(), Some("main"));
    assert!(!main.is_dirty());
}

#[test]
fn list_worktrees_includes_additional_worktree_after_git_worktree_add() {
    let (_tmp, repo) = init_repo();
    let wt_path = repo.parent().unwrap().join("repo-feat");
    git(&repo, &["worktree", "add", "-b", "feat", wt_path.to_str().unwrap()]);

    let items = list_worktrees(&wt(), &repo).expect("list should succeed");
    assert_eq!(items.len(), 2, "expected main + feat worktrees, got {items:?}");

    let branches: Vec<_> = items.iter().filter_map(|i| i.branch.as_deref()).collect();
    assert!(branches.contains(&"main"));
    assert!(branches.contains(&"feat"));

    let feat = items.iter().find(|i| i.branch.as_deref() == Some("feat")).unwrap();
    assert!(!feat.is_main);
    assert!(!feat.is_dirty());
}

#[test]
fn list_worktrees_detects_dirty_untracked_file() {
    let (_tmp, repo) = init_repo();
    std::fs::write(repo.join("new-file.txt"), "hello\n").unwrap();

    let items = list_worktrees(&wt(), &repo).expect("list should succeed");
    let main = items.iter().find(|i| i.is_main).unwrap();
    assert!(main.is_dirty(), "untracked file should mark worktree dirty: {main:?}");
    assert!(
        main.working_tree.as_ref().map(|w| w.untracked).unwrap_or(false),
        "untracked flag should be true"
    );
}

#[test]
fn list_worktrees_detects_dirty_modified_file() {
    let (_tmp, repo) = init_repo();
    std::fs::write(repo.join("tracked.txt"), "v1\n").unwrap();
    git(&repo, &["add", "tracked.txt"]);
    git(&repo, &["commit", "-m", "add tracked"]);
    std::fs::write(repo.join("tracked.txt"), "v2\n").unwrap();

    let items = list_worktrees(&wt(), &repo).expect("list should succeed");
    let main = items.iter().find(|i| i.is_main).unwrap();
    assert!(main.is_dirty());
    assert!(
        main.working_tree.as_ref().map(|w| w.modified).unwrap_or(false),
        "modified flag should be true"
    );
}

#[test]
fn list_worktrees_reports_ahead_on_feature_branch() {
    let (_tmp, repo) = init_repo();
    let feat_path = repo.parent().unwrap().join("repo-feat");
    git(&repo, &["worktree", "add", "-b", "feat", feat_path.to_str().unwrap()]);
    git(&feat_path, &["commit", "--allow-empty", "-m", "ahead-1"]);
    git(&feat_path, &["commit", "--allow-empty", "-m", "ahead-2"]);

    let items = list_worktrees(&wt(), &repo).expect("list should succeed");
    let feat = items.iter().find(|i| i.branch.as_deref() == Some("feat")).unwrap();
    assert_eq!(feat.ahead(), 2, "feat should be 2 commits ahead of main: {feat:?}");
    assert_eq!(feat.behind(), 0);
}

#[test]
fn list_worktrees_reports_locked_worktree() {
    // `wt` flags a worktree as "branch_worktree_mismatch" when the path
    // does not match wt's expected layout, and that state masks the
    // locked flag in the JSON output. We let wt pick the layout by
    // creating the worktree via `wt switch --create` (its default is
    // `{repo_parent}/{project_name}.{branch}` when no user config
    // provides another template). Then we lock it via git and assert
    // via our `list_worktrees` wrapper.

    let (_tmp, repo) = init_repo();
    let wt_bin = wt();
    // Empty HOME so wt ignores the developer's user config (e.g.
    // `~/.config/wt.toml` setting a worktree base path outside the
    // tempdir). With no user config, wt's default places the worktree
    // adjacent to the repo.
    let fake_home = tempfile::tempdir().expect("fake home");

    let out = Command::new(&wt_bin.path)
        .args(["switch", "--create", "feat"])
        .current_dir(&repo)
        .env("HOME", fake_home.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn wt switch");
    assert!(
        out.status.success(),
        "wt switch --create feat failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // wt's default-no-user-config layout places the worktree adjacent
    // to the repo at `{repo_parent}/{repo_name}.{branch}`.
    let feat_path = repo.parent().unwrap().join("repo.feat");
    assert!(
        feat_path.exists(),
        "expected wt to create {feat_path:?}; wt stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    git(&repo, &["worktree", "lock", "--reason", "under test", feat_path.to_str().unwrap()]);

    // Run `wt list` with the same empty HOME so the assertion sees the
    // same layout wt used for setup. (Our production `list_worktrees`
    // wrapper inherits the real HOME, which would flip wt back to
    // mismatch mode. The schema/parse path below is identical to the
    // wrapper's — we only bypass the spawn env.)
    let out = Command::new(&wt_bin.path)
        .args(["list", "--format=json"])
        .current_dir(&repo)
        .env("HOME", fake_home.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn wt list");
    assert!(
        out.status.success(),
        "wt list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let items: Vec<WtItem> = serde_json::from_slice(&out.stdout).expect("parse JSON");

    let feat = items
        .iter()
        .find(|i| i.branch.as_deref() == Some("feat"))
        .expect("feat entry present");
    assert_eq!(
        feat.locked_reason(),
        Some("under test"),
        "locked_reason should round-trip the reason string: {feat:?}"
    );
}

#[test]
fn list_worktrees_errors_on_nonexistent_repo() {
    let nonexistent = Path::new("/tmp/roux-worktrunk-nonexistent-repo-xyz");
    let res = list_worktrees(&wt(), nonexistent);
    assert!(res.is_err(), "listing a nonexistent repo must not succeed");
}

#[test]
fn list_worktrees_handles_repo_with_wt_config_toml() {
    // A .config/wt.toml pre-commit must not break listing.
    let (_tmp, repo) = init_repo();
    std::fs::create_dir_all(repo.join(".config")).unwrap();
    std::fs::write(repo.join(".config").join("wt.toml"), "# empty\n").unwrap();
    assert!(detect_wt_config(&repo));
    let items = list_worktrees(&wt(), &repo).expect("list should still succeed with wt.toml");
    assert_eq!(items.len(), 1);
}

// ---------------------------------------------------------------------------
// create_worktree — real git + real wt
//
// Each create test overrides `HOME` to a tempdir so wt ignores the dev's
// user-level `~/.config/wt.toml` and uses the no-config default layout
// (`{repo_parent}/{project}.{branch}`). This keeps worktrees scoped to
// the test's tempdir and avoids polluting the developer's real homedir.
// ---------------------------------------------------------------------------

/// Build a `CreateOpts` populated with the hermetic env every create
/// test uses. `base` may be overridden by the caller.
fn test_create_opts(home: &Path, base: Option<&'static str>) -> CreateOpts<'static> {
    CreateOpts {
        base,
        env: vec![
            ("HOME".into(), home.as_os_str().into()),
            ("GIT_CONFIG_GLOBAL".into(), std::ffi::OsString::from("/dev/null")),
            ("GIT_CONFIG_SYSTEM".into(), std::ffi::OsString::from("/dev/null")),
            ("GIT_CONFIG_NOSYSTEM".into(), std::ffi::OsString::from("1")),
        ],
    }
}

#[test]
fn create_worktree_creates_new_branch_and_returns_valid_path() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();

    let path = create_worktree(&wt(), &repo, "feat-new", &test_create_opts(fake_home.path(), None))
        .expect("create should succeed");

    assert!(path.is_absolute(), "returned path must be absolute: {path:?}");
    assert!(path.is_dir(), "returned path must exist on disk: {path:?}");
    assert!(
        path.join(".git").exists(),
        "worktree must carry a .git marker: {path:?}"
    );
}

#[test]
fn create_worktree_returns_existing_path_when_branch_already_has_worktree() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let opts = test_create_opts(fake_home.path(), None);

    let first = create_worktree(&wt(), &repo, "feat-noop", &opts).expect("first create");
    let second = create_worktree(&wt(), &repo, "feat-noop", &opts)
        .expect("second create must be a no-op when worktree already exists");

    assert_eq!(first, second, "second call must return the same path (no-op)");
    assert!(first.is_dir());
}

#[test]
fn create_worktree_uses_base_ref_when_supplied() {
    let (_tmp, repo) = init_repo();
    // Give main a second commit so "main" and "HEAD" diverge from initial.
    git(&repo, &["commit", "--allow-empty", "-m", "main-c2"]);
    let main_tip = {
        let out = Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let fake_home = tempfile::tempdir().unwrap();
    let path = create_worktree(
        &wt(),
        &repo,
        "feat-from-main",
        &test_create_opts(fake_home.path(), Some("main")),
    )
    .expect("create with base");

    // HEAD at the new worktree should point at main's tip.
    let wt_head = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(wt_head, main_tip);
}

// ---------------------------------------------------------------------------
// remove_worktree — real git + real wt
// ---------------------------------------------------------------------------

/// Build a RemoveOpts with the hermetic env so tests don't leak into
/// the user's real homedir.
fn test_remove_opts(home: &Path, also_branch: bool) -> RemoveOpts {
    RemoveOpts {
        also_branch,
        force: false,
        env: vec![
            ("HOME".into(), home.as_os_str().into()),
            ("GIT_CONFIG_GLOBAL".into(), std::ffi::OsString::from("/dev/null")),
            ("GIT_CONFIG_SYSTEM".into(), std::ffi::OsString::from("/dev/null")),
            ("GIT_CONFIG_NOSYSTEM".into(), std::ffi::OsString::from("1")),
        ],
    }
}

#[test]
fn remove_worktree_only_leaves_branch() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let wt_bin = wt();

    let path = create_worktree(
        &wt_bin,
        &repo,
        "feat-remove-wt-only",
        &test_create_opts(fake_home.path(), None),
    )
    .expect("create");

    remove_worktree(&wt_bin, &repo, &path, &test_remove_opts(fake_home.path(), false))
        .expect("remove with also_branch=false");

    // Worktree gone on disk.
    assert!(!path.exists(), "worktree path must be gone: {path:?}");

    // Branch still exists in the repo.
    let branches = Command::new("git")
        .args(["branch", "--list"])
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&branches.stdout);
    assert!(
        out.contains("feat-remove-wt-only"),
        "branch should survive `remove --no-delete-branch`: {out}"
    );
}

#[test]
fn remove_worktree_and_branch_drops_both() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let wt_bin = wt();

    let path = create_worktree(
        &wt_bin,
        &repo,
        "feat-remove-both",
        &test_create_opts(fake_home.path(), None),
    )
    .expect("create");

    remove_worktree(&wt_bin, &repo, &path, &test_remove_opts(fake_home.path(), true))
        .expect("remove with also_branch=true");

    assert!(!path.exists());

    let branches = Command::new("git")
        .args(["branch", "--list"])
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&branches.stdout);
    assert!(
        !out.contains("feat-remove-both"),
        "branch should be gone with also_branch=true: {out}"
    );
}

#[test]
fn remove_worktree_returns_typed_error_for_nonexistent_path() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let nonexistent = tempfile::tempdir().unwrap();
    let phantom = nonexistent.path().join("never-created");

    let err = remove_worktree(
        &wt(),
        &repo,
        &phantom,
        &test_remove_opts(fake_home.path(), false),
    )
    .expect_err("removing a path wt doesn't know should fail");

    match err {
        WtError::NotFound { .. } | WtError::NonZeroExit { .. } => {}
        other => panic!("expected NotFound or NonZeroExit, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// diagnostics — real wt
// ---------------------------------------------------------------------------

fn diag_env(home: &Path) -> Vec<(String, std::ffi::OsString)> {
    vec![
        ("HOME".into(), home.as_os_str().into()),
        ("GIT_CONFIG_GLOBAL".into(), std::ffi::OsString::from("/dev/null")),
        ("GIT_CONFIG_SYSTEM".into(), std::ffi::OsString::from("/dev/null")),
        ("GIT_CONFIG_NOSYSTEM".into(), std::ffi::OsString::from("1")),
    ]
}

#[test]
fn list_logs_returns_three_arrays_on_fresh_repo() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let logs = list_logs(&wt(), &repo, &diag_env(fake_home.path()))
        .expect("list_logs on a fresh repo should succeed");
    // Fresh repo has no wt activity yet — all three are empty.
    assert!(logs.command_log.is_empty());
    assert!(logs.hook_output.is_empty());
    assert!(logs.diagnostic.is_empty());
}

#[test]
fn list_logs_still_succeeds_after_wt_activity() {
    // wt writes verbose logs only when run with `-vv`; default-mode
    // activity doesn't guarantee log entries. We exercise the schema
    // end-to-end here by just making sure list_logs keeps succeeding
    // after some wt activity, without asserting any specific entry.
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let _ = create_worktree(&wt(), &repo, "feat-log", &test_create_opts(fake_home.path(), None));
    let _logs = list_logs(&wt(), &repo, &diag_env(fake_home.path()))
        .expect("list_logs after activity should still parse successfully");
}

#[test]
fn show_config_reports_user_path_even_when_absent() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let show = show_config(&wt(), &repo, &diag_env(fake_home.path()))
        .expect("show_config should succeed");
    // User config lives under $HOME; we pointed HOME at a tempdir with
    // no config there, so exists is false but path is reported.
    assert!(!show.user.exists);
    assert!(
        show.user.path.starts_with(fake_home.path().to_str().unwrap()),
        "user config path should live under fake HOME: {:?}",
        show.user.path
    );
    // Project config path points into the repo even when absent.
    assert!(show.project.path.contains(".config/wt.toml"));
}

#[test]
fn show_config_surfaces_project_hook_when_present() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();

    std::fs::create_dir_all(repo.join(".config")).unwrap();
    std::fs::write(
        repo.join(".config").join("wt.toml"),
        "post-start = \"echo hi\"\n",
    )
    .unwrap();

    let show = show_config(&wt(), &repo, &diag_env(fake_home.path()))
        .expect("show_config should succeed");
    assert!(show.project.exists);
    let hooks = extract_hook_defs(&show);
    let post_start = hooks
        .iter()
        .find(|h| h.source == "project" && h.name == "post-start")
        .expect("post-start hook should be extracted");
    assert_eq!(post_start.command, "echo hi");
}

#[test]
fn remove_worktree_refuses_locked_without_force() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();
    let wt_bin = wt();

    let path = create_worktree(
        &wt_bin,
        &repo,
        "feat-locked-remove",
        &test_create_opts(fake_home.path(), None),
    )
    .expect("create");

    git(&repo, &["worktree", "lock", "--reason", "test lock", path.to_str().unwrap()]);

    let err = remove_worktree(
        &wt_bin,
        &repo,
        &path,
        &test_remove_opts(fake_home.path(), false),
    )
    .expect_err("remove of locked worktree must not silently succeed");

    match err {
        WtError::Locked { .. } | WtError::NonZeroExit { .. } => {}
        other => panic!("expected Locked or NonZeroExit, got {other:?}"),
    }
    // Path should still exist — we didn't force.
    assert!(
        path.exists(),
        "locked worktree must not be removed without force"
    );
    // Cleanup
    git(&repo, &["worktree", "unlock", path.to_str().unwrap()]);
    let _ = remove_worktree(&wt_bin, &repo, &path, &test_remove_opts(fake_home.path(), true));
}

#[test]
fn create_worktree_returns_typed_error_on_bogus_base() {
    let (_tmp, repo) = init_repo();
    let fake_home = tempfile::tempdir().unwrap();

    let err = create_worktree(
        &wt(),
        &repo,
        "feat-bad-base",
        &test_create_opts(fake_home.path(), Some("definitely-not-a-branch-xyz")),
    )
    .expect_err("create with nonexistent base should fail");

    // We don't care which variant — only that it's typed and carries the
    // stderr for debugging. Exit status errors land as NonZeroExit.
    match err {
        WtError::NonZeroExit { stderr, .. } => {
            assert!(
                !stderr.is_empty(),
                "stderr should carry the wt error message"
            );
        }
        other => panic!("expected NonZeroExit, got {other:?}"),
    }
}
