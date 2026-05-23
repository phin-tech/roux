//! Multi-scoped notes service.
//!
//! **Experimental.** Vault layout, frontmatter schema, CLI surface, env var
//! names, and Tauri command signatures exposed from this module are all
//! subject to change. See `docs/superpowers/specs/2026-04-18-notes-expansion-design.md`
//! for the full design and stability guarantees (or lack thereof).

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NotesError {
    #[error("invalid topic name")]
    InvalidTopic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Project {
        slug: String,
        name: String,
    },
    Repo {
        slug: String,
        repo_path: String,
        remote: Option<String>,
    },
    Session {
        session_id: String,
        repo_slug: String,
        project_slug: Option<String>,
        branch: String,
        worktree: String,
    },
}

use std::path::{Path, PathBuf};

/// Typed path builder for the vault layout. Pure string manipulation;
/// no filesystem I/O happens here.
#[derive(Debug, Clone)]
pub struct VaultPath {
    root: PathBuf,
}

impl VaultPath {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn global_dir(&self) -> PathBuf {
        self.root.join("global")
    }

    pub fn repo_dir(&self, slug: &str) -> PathBuf {
        self.root.join("repos").join(slug)
    }

    pub fn project_dir(&self, slug: &str) -> PathBuf {
        self.root.join("projects").join(slug)
    }

    pub fn session_dir(&self, slug: &str) -> PathBuf {
        self.root.join("sessions").join(slug)
    }

    /// Resolve the scope-specific directory. `session_slug` is consulted only
    /// when `scope` is `Session`; callers pre-compute it via `session_slug()`.
    pub fn scope_dir(&self, scope: &Scope, session_slug: &str) -> PathBuf {
        match scope {
            Scope::Global => self.global_dir(),
            Scope::Project { slug, .. } => self.project_dir(slug),
            Scope::Repo { slug, .. } => self.repo_dir(slug),
            Scope::Session { .. } => self.session_dir(session_slug),
        }
    }

    /// Path to the scope's anchor `notes.md` (or a named topic file).
    pub fn notes_file(&self, scope: &Scope, topic: Option<&str>, session_slug: &str) -> PathBuf {
        let filename = match topic {
            Some(name) => format!("{name}.md"),
            None => "notes.md".to_string(),
        };
        self.scope_dir(scope, session_slug).join(filename)
    }
}

/// On-disk index mapping canonical `repo_path` and `project_id` values to
/// their vault slugs. Persisted at `<vault_root>/.roux/repos.json` and
/// `<vault_root>/.roux/projects.json`.
///
/// Slugs are frozen once assigned — users rename via dedicated CLI verbs.
pub struct NotesIndex {
    vault_root: std::path::PathBuf,
    repos: std::collections::BTreeMap<String, RepoEntry>,
    projects: std::collections::BTreeMap<String, ProjectEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepoEntry {
    pub slug: String,
    pub remote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectEntry {
    pub slug: String,
    pub name: String,
}

impl NotesIndex {
    /// Load the index from `<vault_root>/.roux/`. Missing files produce an empty index.
    pub fn load(vault_root: &Path) -> Self {
        let repos = read_json_map(&vault_root.join(".roux").join("repos.json"));
        let projects = read_json_map(&vault_root.join(".roux").join("projects.json"));
        Self { vault_root: vault_root.to_path_buf(), repos, projects }
    }

    /// Resolve (and freeze) a slug for the given `repo_path`.
    pub fn resolve_repo(&mut self, repo_path: &str, remote: Option<&str>) -> String {
        if let Some(existing) = self.repos.get(repo_path) {
            return existing.slug.clone();
        }
        let base = remote
            .and_then(slug::slugify_remote_url)
            .or_else(|| slug::slugify_path_basename(repo_path))
            .unwrap_or_else(|| "repo".to_string());
        let slug = self.unique_repo_slug(&base);
        self.repos.insert(
            repo_path.to_string(),
            RepoEntry { slug: slug.clone(), remote: remote.map(|s| s.to_string()) },
        );
        self.persist_repos();
        slug
    }

    /// Resolve (and freeze) a slug for the given `project_id`. Once assigned,
    /// the slug never changes even if the project's name later changes.
    pub fn resolve_project(&mut self, project_id: &str, project_name: &str) -> String {
        if let Some(existing) = self.projects.get(project_id) {
            return existing.slug.clone();
        }
        let sanitized = project_name.replace(['/', '\\'], " ");
        let base = topic::slugify(&sanitized).unwrap_or_else(|_| "project".to_string());
        let slug = self.unique_project_slug(&base);
        self.projects.insert(
            project_id.to_string(),
            ProjectEntry { slug: slug.clone(), name: project_name.to_string() },
        );
        self.persist_projects();
        slug
    }

    fn unique_project_slug(&self, base: &str) -> String {
        let used: std::collections::HashSet<&str> =
            self.projects.values().map(|v| v.slug.as_str()).collect();
        if !used.contains(base) {
            return base.to_string();
        }
        let mut n: u32 = 2;
        loop {
            let candidate = format!("{base}-{n}");
            if !used.contains(candidate.as_str()) {
                return candidate;
            }
            n += 1;
        }
    }

    fn persist_projects(&self) {
        write_json_map(&self.vault_root.join(".roux").join("projects.json"), &self.projects);
    }

    fn unique_repo_slug(&self, base: &str) -> String {
        let used: std::collections::HashSet<&str> =
            self.repos.values().map(|v| v.slug.as_str()).collect();
        if !used.contains(base) {
            return base.to_string();
        }
        let mut n: u32 = 2;
        loop {
            let candidate = format!("{base}-{n}");
            if !used.contains(candidate.as_str()) {
                return candidate;
            }
            n += 1;
        }
    }

    fn persist_repos(&self) {
        write_json_map(&self.vault_root.join(".roux").join("repos.json"), &self.repos);
    }
}

fn read_json_map<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> std::collections::BTreeMap<String, T> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => std::collections::BTreeMap::new(),
    }
}

fn write_json_map<T: serde::Serialize>(path: &Path, map: &std::collections::BTreeMap<String, T>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(path, s);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AppendOpts<'a> {
    Plain,
    Timestamped { timestamp: &'a str, id: &'a str, include_web_anchor: bool },
}

/// Top-level service binding together `VaultPath` + `NotesIndex` +
/// frontmatter/entry/tag primitives. Command handlers and CLI both
/// go through here; no business logic lives above this layer.
pub struct NotesService {
    vault: VaultPath,
    index: NotesIndex,
}

impl NotesService {
    pub fn new(vault_root: impl Into<PathBuf>) -> Self {
        let vault_root = vault_root.into();
        let vault = VaultPath::new(vault_root.clone());
        let index = NotesIndex::load(&vault_root);
        Self { vault, index }
    }

    /// Resolve the repo slug, freezing it in the index on first call.
    /// Delegates to `NotesIndex::resolve_repo`.
    pub fn freeze_repo_slug(&mut self, repo_path: &str, remote: Option<&str>) -> String {
        self.index.resolve_repo(repo_path, remote)
    }

    /// Resolve the project slug, freezing it in the index on first call.
    /// Delegates to `NotesIndex::resolve_project`.
    pub fn freeze_project_slug(&mut self, project_id: &str, project_name: &str) -> String {
        self.index.resolve_project(project_id, project_name)
    }

    /// Convert a scope name + session context into a `(Scope, topic, session_slug)`
    /// tuple that the file-level primitives can consume. Resolves (and freezes)
    /// the repo/project slugs via `NotesIndex` on the way through.
    ///
    /// - `scope_name` is one of `"global" | "project" | "repo" | "session"`.
    /// - `session` is required for project/repo/session scopes.
    /// - `project_name` must be supplied when the caller needs the project
    ///   scope and the session has a `project_id`.
    /// - `remote` is the git origin URL (if any) for the session's repo.
    pub fn resolve_target(
        &mut self,
        scope_name: &str,
        session: Option<&roux_core::Session>,
        project_name: Option<&str>,
        remote: Option<&str>,
        topic: Option<String>,
    ) -> Result<(Scope, Option<String>, String), String> {
        let session_slug_str = match session {
            Some(s) => session_slug(&s.branch, &s.id),
            None => "no-session".to_string(),
        };

        let scope = match scope_name {
            "global" => Scope::Global,
            "project" => {
                let s = session.ok_or_else(|| "session required for project scope".to_string())?;
                let project_id = s
                    .project_id
                    .as_deref()
                    .ok_or_else(|| "no project assigned to this session".to_string())?;
                let name = project_name.ok_or_else(|| "project name not provided".to_string())?;
                let slug = self.index.resolve_project(project_id, name);
                Scope::Project { slug, name: name.to_string() }
            }
            "repo" => {
                let s = session.ok_or_else(|| "session required for repo scope".to_string())?;
                let slug = self.index.resolve_repo(&s.repo_root, remote);
                Scope::Repo {
                    slug,
                    repo_path: s.repo_root.clone(),
                    remote: remote.map(|r| r.to_string()),
                }
            }
            "session" => {
                let s = session.ok_or_else(|| "session required for session scope".to_string())?;
                let repo_slug = self.index.resolve_repo(&s.repo_root, remote);
                let project_slug = match s.project_id.as_deref() {
                    Some(pid) => project_name.map(|n| self.index.resolve_project(pid, n)),
                    None => None,
                };
                Scope::Session {
                    session_id: s.id.clone(),
                    repo_slug,
                    project_slug,
                    branch: s.branch.clone(),
                    worktree: s.worktree_path.clone(),
                }
            }
            other => return Err(format!("unknown scope '{other}'")),
        };

        Ok((scope, topic, session_slug_str))
    }

    /// Path to the scope/topic file (without touching the filesystem).
    pub fn file_path(&self, scope: &Scope, topic: Option<&str>, session_slug: &str) -> PathBuf {
        self.vault.notes_file(scope, topic, session_slug)
    }

    /// Path to the scope's directory (without touching the filesystem).
    pub fn dir_path(&self, scope: &Scope, session_slug: &str) -> PathBuf {
        self.vault.scope_dir(scope, session_slug)
    }

    /// Tag-search across the vault, delegating to the free `search_by_tags`.
    pub fn search(&self, scope_filter: Option<&str>, tags: &[String], exact: bool) -> Vec<PathBuf> {
        search_by_tags(self.vault.root(), scope_filter, tags, exact)
    }

    /// Append `content` to the scope/topic file, preserving its existing
    /// frontmatter. `opts` controls whether it's a plain append (just a
    /// leading newline + `content`) or a timestamped entry block.
    pub fn append_file(
        &mut self,
        scope: &Scope,
        topic: Option<&str>,
        session_slug: &str,
        content: &str,
        opts: AppendOpts<'_>,
        now: &str,
        extra_tags: &[String],
    ) -> std::io::Result<()> {
        let addition = match opts {
            AppendOpts::Plain => format!("\n{content}"),
            AppendOpts::Timestamped { timestamp, id, include_web_anchor } => {
                timestamped_entry::format(content, timestamp, id, include_web_anchor)
            }
        };

        let existing = self.read_file(scope, topic, session_slug)?;
        let (old_fm, old_body) = frontmatter::split(&existing);
        let new_body = format!("{old_body}{addition}");
        let stub = match old_fm {
            Some(src) => format!("---\n{src}\n---\n{new_body}"),
            None => new_body,
        };
        let new_contents = frontmatter::ensure(&stub, scope, now, extra_tags);

        let path = self.vault.notes_file(scope, topic, session_slug);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, new_contents)?;
        Ok(())
    }

    /// Write `body` (a markdown body without frontmatter) to the scope/topic
    /// file, preserving any existing frontmatter fields (including `created`
    /// and unknown user fields) and refreshing `updated` to `now`.
    /// `extra_tags` are union-merged with the scope's default tag list.
    pub fn write_file(
        &mut self,
        scope: &Scope,
        topic: Option<&str>,
        session_slug: &str,
        body: &str,
        now: &str,
        extra_tags: &[String],
    ) -> std::io::Result<()> {
        let path = self.vault.notes_file(scope, topic, session_slug);

        // If a file already exists, pull its existing frontmatter forward by
        // feeding ensure a stub that combines the old frontmatter + the new
        // body. ensure will preserve `created`, unknown fields, etc.
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let (old_fm, _old_body) = frontmatter::split(&existing);
        let stub = match old_fm {
            Some(src) => format!("---\n{src}\n---\n{body}"),
            None => body.to_string(),
        };
        let new_contents = frontmatter::ensure(&stub, scope, now, extra_tags);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, new_contents)?;
        Ok(())
    }

    /// Read the raw file contents at the scope/topic. Returns empty string
    /// if the file doesn't exist (anchor files are lazy-materialized on
    /// first write).
    pub fn read_file(
        &self,
        scope: &Scope,
        topic: Option<&str>,
        session_slug: &str,
    ) -> std::io::Result<String> {
        let path = self.vault.notes_file(scope, topic, session_slug);
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e),
        }
    }
}

/// Walk `vault_root` and return absolute paths of `.md` files whose tags
/// (frontmatter `tags:` list, union with inline `#tag` occurrences in the
/// body) match all of `required_tags`.
///
/// With `exact = false` (default for the CLI), hierarchical prefix matching
/// is used: `--tag api` matches `api`, `api/tls`, `api/foo`. With `exact =
/// true`, only literal matches count.
///
/// `scope_filter` restricts the walk to one top-level scope folder
/// (`"global" | "project" | "repo" | "session"`). `None` walks the whole
/// vault.
pub fn search_by_tags(
    vault_root: &Path,
    scope_filter: Option<&str>,
    required_tags: &[String],
    exact: bool,
) -> Vec<std::path::PathBuf> {
    let search_root = match scope_filter {
        Some(scope) => match scope {
            "global" => vault_root.join("global"),
            "project" => vault_root.join("projects"),
            "repo" => vault_root.join("repos"),
            "session" => vault_root.join("sessions"),
            _ => vault_root.to_path_buf(),
        },
        None => vault_root.to_path_buf(),
    };

    let mut hits = Vec::new();
    walk_md_files(&search_root, &mut |path| {
        if file_matches_all_tags(path, required_tags, exact) {
            hits.push(path.to_path_buf());
        }
    });
    hits.sort();
    hits
}

fn walk_md_files(root: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip dot-folders (like .roux) — Obsidian does too.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        if path.is_dir() {
            walk_md_files(&path, visit);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            visit(&path);
        }
    }
}

fn file_matches_all_tags(path: &Path, required: &[String], exact: bool) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let (fm_src, body) = frontmatter::split(&contents);
    let mut all_tags: Vec<String> = Vec::new();
    if let Some(src) = fm_src {
        if let Ok(map) = serde_yaml::from_str::<serde_yaml::Mapping>(src) {
            if let Some(serde_yaml::Value::Sequence(seq)) =
                map.get(serde_yaml::Value::String("tags".into()))
            {
                for v in seq {
                    if let Some(s) = v.as_str() {
                        all_tags.push(s.to_string());
                    }
                }
            }
        }
    }
    all_tags.extend(inline_tags::parse(body));

    required.iter().all(|needed| {
        all_tags.iter().any(|t| if exact { t == needed } else { tag_matches_prefix(t, needed) })
    })
}

/// `haystack` matches `needle` under hierarchical prefix rules iff
/// `haystack == needle` or `haystack` starts with `needle/`.
fn tag_matches_prefix(haystack: &str, needle: &str) -> bool {
    haystack == needle
        || (haystack.starts_with(needle) && haystack[needle.len()..].starts_with('/'))
}

/// Build a session folder slug (`<branch-slug>--<short-id>`).
/// - `branch` is slugified (lowercase, `/` → `-`, non-alphanumerics → `-`,
///   collapsed+trimmed). An empty branch falls back to `detached`.
/// - `short_id` is the first 6 ASCII alphanumeric characters of `session_id`
///   (skipping `-` etc., since session ids are uuids).
pub fn session_slug(branch: &str, session_id: &str) -> String {
    let branch_part = if branch.is_empty() {
        "detached".to_string()
    } else {
        let sanitized = branch.replace(['/', '\\'], " ");
        topic::slugify(&sanitized).unwrap_or_else(|_| "detached".to_string())
    };
    let short: String = session_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(6).collect();
    format!("{branch_part}--{short}")
}

pub mod slug {
    /// Derive a slug from a filesystem path's basename.
    ///
    /// Returns `None` if the basename is empty after slugification.
    pub fn slugify_path_basename(path: &str) -> Option<String> {
        let trimmed = path.trim_end_matches('/');
        let basename = trimmed.rsplit('/').next().unwrap_or("");
        super::topic::slugify(basename).ok()
    }

    /// Derive a repo slug from a git remote URL.
    ///
    /// Strips common schemes (`ssh://`, `https://`, `git://`, etc.),
    /// SSH user prefix, host, and `.git` suffix, then slugifies the
    /// resulting owner/repo path (lowercase, `/` → `-`, non-alphanumerics
    /// collapsed to single `-`, trimmed).
    ///
    /// Returns `None` if the result would be empty.
    pub fn slugify_remote_url(url: &str) -> Option<String> {
        // Pull out the owner/repo path piece.
        let after_scheme = strip_scheme(url);
        let after_userhost = strip_userhost(after_scheme);
        let without_git = after_userhost.strip_suffix(".git").unwrap_or(after_userhost);

        let mut out = String::with_capacity(without_git.len());
        let mut prev_dash = true;
        for ch in without_git.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        if out.ends_with('-') {
            out.pop();
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn strip_scheme(url: &str) -> &str {
        for scheme in ["ssh://", "https://", "http://", "git://", "file://"] {
            if let Some(r) = url.strip_prefix(scheme) {
                return r;
            }
        }
        url
    }

    /// Remove the `user@host:` (SSH) or `host/` (HTTP) prefix, leaving only
    /// the owner/repo path. Works for both `git@github.com:owner/repo` and
    /// `github.com/owner/repo`.
    fn strip_userhost(s: &str) -> &str {
        if let Some((_, rest)) = s.split_once(':') {
            // SSH shorthand (git@host:path) or http://host:port/path — take path.
            rest
        } else if let Some((_, rest)) = s.split_once('/') {
            rest
        } else {
            s
        }
    }
}

pub mod frontmatter {
    use super::Scope;

    /// Split a file's contents into optional frontmatter YAML source and body.
    /// Frontmatter is the `---`-delimited YAML block at the very top of the file.
    pub fn split(contents: &str) -> (Option<&str>, &str) {
        let rest = match contents.strip_prefix("---\n") {
            Some(r) => r,
            None => return (None, contents),
        };
        if let Some(end) = rest.find("\n---\n") {
            let (fm, after) = rest.split_at(end);
            let body = &after["\n---\n".len()..];
            (Some(fm), body)
        } else {
            (None, contents)
        }
    }

    /// Write or update a file's frontmatter for the given scope.
    pub fn ensure(contents: &str, scope: &Scope, now: &str, extra_tags: &[String]) -> String {
        use serde_yaml::{Mapping, Value};

        let (fm_src, body) = split(contents);
        let mut map: Mapping = match fm_src {
            Some(src) => serde_yaml::from_str(src).unwrap_or_default(),
            None => Mapping::new(),
        };

        let (scope_type, default_tag) = match scope {
            Scope::Global => ("global", "roux/global"),
            Scope::Project { .. } => ("project", "roux/project"),
            Scope::Repo { .. } => ("repo", "roux/repo"),
            Scope::Session { .. } => ("session", "roux/session"),
        };
        map.insert(Value::String("type".into()), Value::String(scope_type.into()));
        match scope {
            Scope::Global => {}
            Scope::Project { slug, name } => {
                map.insert(Value::String("project".into()), Value::String(slug.clone()));
                map.insert(Value::String("project_name".into()), Value::String(name.clone()));
            }
            Scope::Repo { slug, repo_path, remote } => {
                map.insert(Value::String("repo".into()), Value::String(slug.clone()));
                map.insert(Value::String("repo_path".into()), Value::String(repo_path.clone()));
                match remote {
                    Some(r) => {
                        map.insert(Value::String("remote".into()), Value::String(r.clone()));
                    }
                    None => {
                        map.remove(Value::String("remote".into()));
                    }
                }
            }
            Scope::Session { session_id, repo_slug, project_slug, branch, worktree } => {
                map.insert(Value::String("session_id".into()), Value::String(session_id.clone()));
                map.insert(Value::String("repo".into()), Value::String(repo_slug.clone()));
                map.insert(
                    Value::String("project".into()),
                    match project_slug {
                        Some(p) => Value::String(p.clone()),
                        None => Value::Null,
                    },
                );
                map.insert(Value::String("branch".into()), Value::String(branch.clone()));
                map.insert(Value::String("worktree".into()), Value::String(worktree.clone()));
            }
        }

        let tags_key = Value::String("tags".into());
        let mut existing: Vec<String> = match map.get(&tags_key) {
            Some(Value::Sequence(seq)) => {
                seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
            }
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        };
        if !existing.iter().any(|t| t == default_tag) {
            existing.insert(0, default_tag.to_string());
        }
        for t in extra_tags {
            if !existing.iter().any(|e| e == t) {
                existing.push(t.clone());
            }
        }
        let seq: Vec<Value> = existing.into_iter().map(Value::String).collect();
        map.insert(tags_key, Value::Sequence(seq));

        let created_key = Value::String("created".into());
        if !map.contains_key(&created_key) {
            map.insert(created_key, Value::String(now.into()));
        }
        map.insert(Value::String("updated".into()), Value::String(now.into()));

        let yaml = serde_yaml::to_string(&map).expect("frontmatter serialize");
        format!("---\n{yaml}---\n{body}")
    }
}

/// One-shot migration: for each `<project_id>.txt` in the legacy notes
/// directory, write its content into the vault's `projects/<slug>/notes.md`
/// with proper frontmatter. Leaves the legacy files in place as a backup.
///
/// Returns the number of files migrated. Non-destructive; idempotent:
/// target files that already exist in the vault are skipped.
///
/// `project_name_lookup` resolves a project id to its display name. The
/// migration uses it to freeze the project's vault slug via the index.
pub fn migrate_legacy_project_notes(
    legacy_notes_dir: &Path,
    project_name_lookup: &dyn Fn(&str) -> Option<String>,
    svc: &mut NotesService,
    now: &str,
) -> usize {
    let mut migrated = 0usize;
    let Ok(entries) = std::fs::read_dir(legacy_notes_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let Some(project_id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(name) = project_name_lookup(project_id) else {
            continue;
        };
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let slug = svc.freeze_project_slug(project_id, &name);
        let scope = Scope::Project { slug, name };
        // Skip if vault file already exists (idempotency).
        let target = svc.file_path(&scope, None, "no-session");
        if target.exists() {
            continue;
        }
        if svc.write_file(&scope, None, "no-session", &body, now, &[]).is_ok() {
            migrated += 1;
        }
    }
    migrated
}

pub mod inline_tags {
    /// Extract inline `#tag` occurrences from a markdown body.
    ///
    /// Obsidian-compatible rules:
    /// - `#` only starts a tag at line start or after whitespace (excludes
    ///   URL fragments like `https://example.com/#anchor`).
    /// - Tag chars: ASCII alphanumerics, `-`, `_`, `/` (hierarchical).
    /// - Purely numeric tags (e.g. `#123`) are not tags — matches Obsidian.
    /// - Inline code spans (`` `...` ``) are skipped.
    /// - Fenced code blocks (``` ```...``` ```) are skipped.
    /// - Indented (4+ spaces or tab) code blocks are skipped.
    pub fn parse(body: &str) -> Vec<String> {
        let mut tags = Vec::new();
        let mut in_fence = false;
        for line in body.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            if line.starts_with("    ") || line.starts_with('\t') {
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '#' && (i == 0 || chars[i - 1].is_whitespace()) {
                    let start = i + 1;
                    let mut end = start;
                    while end < chars.len() && is_tag_char(chars[end]) {
                        end += 1;
                    }
                    if end > start {
                        let tag: String = chars[start..end].iter().collect();
                        if !tag.chars().all(|c| c.is_ascii_digit()) {
                            tags.push(tag);
                        }
                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
        }
        tags
    }

    fn is_tag_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/'
    }
}

pub mod timestamped_entry {
    /// Format a timestamped append entry block.
    ///
    /// Pure function. Caller supplies timestamp (`YYYY-MM-DD HH:MM`) and
    /// id (8 hex chars); this module doesn't touch the clock or RNG.
    pub fn format(content: &str, timestamp: &str, id: &str, include_web_anchor: bool) -> String {
        let mut out = String::new();
        out.push('\n');
        if include_web_anchor {
            out.push_str(&format!("<a id=\"entry-{id}\"></a>\n\n"));
        }
        out.push_str(&format!("## {timestamp}\n\n{content}\n\n^entry-{id}\n"));
        out
    }
}

pub mod topic {
    use super::NotesError;

    pub fn slugify(name: &str) -> Result<String, NotesError> {
        if name.contains('/') || name.contains('\\') {
            return Err(NotesError::InvalidTopic);
        }
        let mut out = String::with_capacity(name.len());
        let mut prev_dash = true;
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        if out.ends_with('-') {
            out.pop();
        }
        if out.is_empty() {
            return Err(NotesError::InvalidTopic);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_replaces_spaces() {
        let got = topic::slugify("API Gotchas").unwrap();
        assert_eq!(got, "api-gotchas");
    }

    #[test]
    fn slugify_collapses_runs_and_trims() {
        let got = topic::slugify("  api   gotchas  ").unwrap();
        assert_eq!(got, "api-gotchas");
    }

    #[test]
    fn slugify_rejects_path_separators() {
        assert_eq!(topic::slugify("foo/bar"), Err(NotesError::InvalidTopic));
        assert_eq!(topic::slugify("foo\\bar"), Err(NotesError::InvalidTopic));
    }

    #[test]
    fn slugify_rejects_empty_and_all_punctuation() {
        assert_eq!(topic::slugify(""), Err(NotesError::InvalidTopic));
        assert_eq!(topic::slugify("   "), Err(NotesError::InvalidTopic));
        assert_eq!(topic::slugify("..."), Err(NotesError::InvalidTopic));
    }

    #[test]
    fn timestamped_entry_with_web_anchor() {
        let got = timestamped_entry::format(
            "retried after clearing token cache, still 401",
            "2026-04-18 14:30",
            "a1b2c3d4",
            true,
        );
        let expected = "\n<a id=\"entry-a1b2c3d4\"></a>\n\n## 2026-04-18 14:30\n\nretried after clearing token cache, still 401\n\n^entry-a1b2c3d4\n";
        assert_eq!(got, expected);
    }

    #[test]
    fn timestamped_entry_without_web_anchor() {
        let got = timestamped_entry::format("fix shipped", "2026-04-18 14:30", "a1b2c3d4", false);
        let expected = "\n## 2026-04-18 14:30\n\nfix shipped\n\n^entry-a1b2c3d4\n";
        assert_eq!(got, expected);
    }

    #[test]
    fn inline_tags_finds_simple_tag_in_prose() {
        assert_eq!(inline_tags::parse("random #hello prose"), vec!["hello"]);
    }

    #[test]
    fn inline_tags_skips_fenced_code_blocks() {
        let body = "before #real\n```\n#nope\n```\n#after";
        assert_eq!(inline_tags::parse(body), vec!["real", "after"]);
    }

    #[test]
    fn inline_tags_skips_indented_code_blocks() {
        let body = "normal #yes\n    #nope indented 4 spaces\n\t#nope tab\n#after";
        assert_eq!(inline_tags::parse(body), vec!["yes", "after"]);
    }

    #[test]
    fn inline_tags_rejects_purely_numeric_tags() {
        assert_eq!(inline_tags::parse("ticket #123 and tag #v2"), vec!["v2"]);
    }

    #[test]
    fn inline_tags_handles_hierarchical_urls_and_inline_code() {
        // ratcheting: URL fragment excluded (preceded by `/`, not whitespace),
        // inline code excluded (preceded by backtick), hierarchical tag kept.
        let body = "see #api/tls docs at https://example.com/#frag and `#notatag` in code";
        assert_eq!(inline_tags::parse(body), vec!["api/tls"]);
    }

    #[test]
    fn frontmatter_split_extracts_yaml_and_body() {
        let input = "---\ntype: repo\nfoo: bar\n---\nbody content here\n";
        let (fm, body) = frontmatter::split(input);
        assert_eq!(fm, Some("type: repo\nfoo: bar"));
        assert_eq!(body, "body content here\n");
    }

    #[test]
    fn frontmatter_split_returns_none_when_absent() {
        let input = "just a body\n";
        let (fm, body) = frontmatter::split(input);
        assert_eq!(fm, None);
        assert_eq!(body, "just a body\n");
    }

    fn parsed_fm(s: &str) -> serde_yaml::Mapping {
        let (fm, _body) = frontmatter::split(s);
        serde_yaml::from_str(fm.expect("frontmatter present")).expect("valid yaml")
    }

    #[test]
    fn frontmatter_ensure_writes_global_scope_on_empty_input() {
        let got = frontmatter::ensure("", &Scope::Global, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert_eq!(map["type"].as_str(), Some("global"));
        assert_eq!(map["created"].as_str(), Some("2026-04-18T14:30:00-05:00"));
        assert_eq!(map["updated"].as_str(), Some("2026-04-18T14:30:00-05:00"));
        let tags = map["tags"].as_sequence().expect("tags is sequence");
        assert_eq!(tags.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(), vec!["roux/global"]);
        let (_fm, body) = frontmatter::split(&got);
        assert_eq!(body, "");
    }

    #[test]
    fn frontmatter_ensure_preserves_existing_created_and_body() {
        let input = "---\ntype: global\ntags:\n- roux/global\ncreated: 2026-04-17T09:00:00-05:00\nupdated: 2026-04-17T09:00:00-05:00\n---\nexisting body text\n";
        let got = frontmatter::ensure(input, &Scope::Global, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert_eq!(map["created"].as_str(), Some("2026-04-17T09:00:00-05:00"));
        assert_eq!(map["updated"].as_str(), Some("2026-04-18T14:30:00-05:00"));
        let (_fm, body) = frontmatter::split(&got);
        assert_eq!(body, "existing body text\n");
    }

    #[test]
    fn frontmatter_ensure_preserves_unknown_user_fields() {
        let input = "---\ntype: global\nfoo: bar\ncustom_list:\n- one\n- two\ncreated: 2026-04-17T09:00:00-05:00\n---\n";
        let got = frontmatter::ensure(input, &Scope::Global, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert_eq!(map["foo"].as_str(), Some("bar"));
        let list: Vec<&str> =
            map["custom_list"].as_sequence().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(list, vec!["one", "two"]);
    }

    #[test]
    fn frontmatter_ensure_union_merges_extra_tags_with_existing() {
        let input = "---\ntype: global\ntags:\n- roux/global\n- api\ncreated: 2026-04-17T09:00:00-05:00\nupdated: 2026-04-17T09:00:00-05:00\n---\n";
        let got = frontmatter::ensure(
            input,
            &Scope::Global,
            "2026-04-18T14:30:00-05:00",
            &["tls".to_string(), "api".to_string()], // api is dup; should not be duplicated
        );
        let map = parsed_fm(&got);
        let tags: Vec<&str> =
            map["tags"].as_sequence().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(tags, vec!["roux/global", "api", "tls"]);
    }

    #[test]
    fn frontmatter_ensure_writes_repo_scope_fields() {
        let scope = Scope::Repo {
            slug: "phin-tech-roux".to_string(),
            repo_path: "/Users/sam/src/github.com/phin-tech/roux".to_string(),
            remote: Some("git@github.com:phin-tech/roux.git".to_string()),
        };
        let got = frontmatter::ensure("", &scope, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert_eq!(map["type"].as_str(), Some("repo"));
        assert_eq!(map["repo"].as_str(), Some("phin-tech-roux"));
        assert_eq!(map["repo_path"].as_str(), Some("/Users/sam/src/github.com/phin-tech/roux"));
        assert_eq!(map["remote"].as_str(), Some("git@github.com:phin-tech/roux.git"));
    }

    #[test]
    fn slug_remote_ssh_form() {
        assert_eq!(
            slug::slugify_remote_url("git@github.com:phin-tech/roux.git"),
            Some("phin-tech-roux".to_string())
        );
    }

    #[test]
    fn slug_remote_https_form() {
        assert_eq!(
            slug::slugify_remote_url("https://github.com/phin-tech/roux.git"),
            Some("phin-tech-roux".to_string())
        );
    }

    #[test]
    fn slug_remote_nested_path() {
        assert_eq!(
            slug::slugify_remote_url("https://gitlab.example.com/org/sub/repo"),
            Some("org-sub-repo".to_string())
        );
    }

    #[test]
    fn slug_remote_empty_after_strip_returns_none() {
        assert_eq!(slug::slugify_remote_url(""), None);
    }

    #[test]
    fn slug_path_basename_simple() {
        assert_eq!(
            slug::slugify_path_basename("/Users/sam/src/playground"),
            Some("playground".to_string())
        );
    }

    #[test]
    fn slug_path_basename_trailing_slash() {
        assert_eq!(
            slug::slugify_path_basename("/Users/sam/src/my repo/"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn slug_path_basename_empty_returns_none() {
        assert_eq!(slug::slugify_path_basename(""), None);
        assert_eq!(slug::slugify_path_basename("/"), None);
    }

    #[test]
    fn index_resolve_repo_uses_remote_slug_and_is_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let mut idx = NotesIndex::load(tmp.path());
        let s1 = idx.resolve_repo("/Users/sam/src/roux", Some("git@github.com:phin-tech/roux.git"));
        assert_eq!(s1, "phin-tech-roux");
        let s2 = idx.resolve_repo("/Users/sam/src/roux", Some("git@github.com:phin-tech/roux.git"));
        assert_eq!(s2, "phin-tech-roux");
    }

    #[test]
    fn index_resolve_repo_adds_suffix_on_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut idx = NotesIndex::load(tmp.path());
        // Two distinct repo_paths, same derived slug (both basename "roux", no remote)
        let a = idx.resolve_repo("/Users/sam/src/a/roux", None);
        let b = idx.resolve_repo("/Users/sam/src/b/roux", None);
        let c = idx.resolve_repo("/Users/sam/src/c/roux", None);
        assert_eq!(a, "roux");
        assert_eq!(b, "roux-2");
        assert_eq!(c, "roux-3");
    }

    #[test]
    fn index_falls_back_to_path_when_no_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let mut idx = NotesIndex::load(tmp.path());
        let s = idx.resolve_repo("/Users/sam/src/playground", None);
        assert_eq!(s, "playground");
    }

    fn write_note(root: &Path, rel: &str, content: &str) -> std::path::PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn mock_session() -> roux_core::Session {
        roux_core::Session {
            id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
            name: "my session".to_string(),
            repo_root: "/Users/sam/src/roux".to_string(),
            worktree_path: "/Users/sam/src/worktrees/feature".to_string(),
            branch: "feature/session-notes".to_string(),
            is_worktree: true,
            status: roux_core::SessionStatus::Disconnected,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: true,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: None,
            pinned_pr_url: None,
            smol_machine_name: None,
        }
    }

    #[test]
    fn resolve_target_global_ignores_session_context() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        let (scope, topic, _session_slug) =
            svc.resolve_target("global", None, None, None, Some("my-topic".to_string())).unwrap();
        assert_eq!(scope, Scope::Global);
        assert_eq!(topic.as_deref(), Some("my-topic"));
    }

    #[test]
    fn resolve_target_repo_uses_remote_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        let session = mock_session();
        let (scope, _topic, _session_slug) = svc
            .resolve_target(
                "repo",
                Some(&session),
                None,
                Some("git@github.com:phin-tech/roux.git"),
                None,
            )
            .unwrap();
        match scope {
            Scope::Repo { slug, .. } => assert_eq!(slug, "phin-tech-roux"),
            _ => panic!("expected repo scope"),
        }
    }

    #[test]
    fn resolve_target_project_requires_session_with_project_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        let session = mock_session(); // project_id = None
        let err = svc
            .resolve_target("project", Some(&session), Some("Anything"), None, None)
            .unwrap_err();
        assert!(err.contains("project"));
    }

    #[test]
    fn resolve_target_project_when_session_has_one() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        let mut session = mock_session();
        session.project_id = Some("proj-1".to_string());
        let (scope, _topic, _session_slug) = svc
            .resolve_target("project", Some(&session), Some("Marketing Revamp"), None, None)
            .unwrap();
        match scope {
            Scope::Project { slug, name } => {
                assert_eq!(slug, "marketing-revamp");
                assert_eq!(name, "Marketing Revamp");
            }
            _ => panic!("expected project scope"),
        }
    }

    #[test]
    fn resolve_target_session_produces_session_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        let session = mock_session();
        let (scope, _topic, session_slug) = svc
            .resolve_target(
                "session",
                Some(&session),
                None,
                Some("git@github.com:phin-tech/roux.git"),
                None,
            )
            .unwrap();
        assert_eq!(session_slug, "feature-session-notes--a1b2c3");
        match scope {
            Scope::Session { branch, repo_slug, project_slug, .. } => {
                assert_eq!(branch, "feature/session-notes");
                assert_eq!(repo_slug, "phin-tech-roux");
                assert_eq!(project_slug, None);
            }
            _ => panic!("expected session scope"),
        }
    }

    #[test]
    fn migrate_moves_txt_files_into_vault_with_frontmatter() {
        let legacy = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();

        std::fs::write(legacy.path().join("proj-1.txt"), "first body\n").unwrap();
        std::fs::write(legacy.path().join("proj-2.txt"), "second body\n").unwrap();
        // Non-txt file should be ignored.
        std::fs::write(legacy.path().join("README"), "ignored").unwrap();

        let mut svc = NotesService::new(vault.path());
        let lookup = |pid: &str| match pid {
            "proj-1" => Some("Marketing Revamp".to_string()),
            "proj-2" => Some("Hiring Pipeline".to_string()),
            _ => None,
        };
        let count = migrate_legacy_project_notes(
            legacy.path(),
            &lookup,
            &mut svc,
            "2026-04-18T10:00:00-05:00",
        );
        assert_eq!(count, 2);

        // Migrated files exist in the vault with frontmatter + body.
        let p1 = vault.path().join("projects/marketing-revamp/notes.md");
        let p2 = vault.path().join("projects/hiring-pipeline/notes.md");
        assert!(p1.exists());
        assert!(p2.exists());
        let c1 = std::fs::read_to_string(&p1).unwrap();
        assert!(c1.starts_with("---\n"));
        assert!(c1.contains("first body"));

        // Legacy .txt files are left in place as backup.
        assert!(legacy.path().join("proj-1.txt").exists());
    }

    #[test]
    fn migrate_is_idempotent_when_vault_file_already_exists() {
        let legacy = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(legacy.path().join("proj-1.txt"), "from legacy").unwrap();

        let mut svc = NotesService::new(vault.path());
        let lookup = |_: &str| Some("Marketing Revamp".to_string());

        // First run migrates.
        let first =
            migrate_legacy_project_notes(legacy.path(), &lookup, &mut svc, "2026-04-18T10:00");
        assert_eq!(first, 1);
        // Second run finds the vault file already exists and skips.
        let second =
            migrate_legacy_project_notes(legacy.path(), &lookup, &mut svc, "2026-04-18T10:05");
        assert_eq!(second, 0);
    }

    #[test]
    fn migrate_skips_unknown_projects() {
        let legacy = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(legacy.path().join("unknown.txt"), "orphaned").unwrap();

        let mut svc = NotesService::new(vault.path());
        let lookup = |_: &str| None;
        let count =
            migrate_legacy_project_notes(legacy.path(), &lookup, &mut svc, "2026-04-18T10:00");
        assert_eq!(count, 0);
    }

    #[test]
    fn session_slug_combines_branch_and_short_id() {
        assert_eq!(
            session_slug("feature/session-notes", "a1b2c3d4-e5f6-7890-abcd-ef1234567890"),
            "feature-session-notes--a1b2c3"
        );
    }

    #[test]
    fn session_slug_falls_back_to_detached_on_empty_branch() {
        assert_eq!(session_slug("", "abcdef123"), "detached--abcdef");
    }

    #[test]
    fn service_read_missing_file_returns_empty_string() {
        let tmp = tempfile::tempdir().unwrap();
        let svc = NotesService::new(tmp.path());
        let got = svc.read_file(&Scope::Global, None, "unused").unwrap();
        assert_eq!(got, "");
    }

    #[test]
    fn service_append_plain_creates_file_with_frontmatter_and_body() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        svc.append_file(
            &Scope::Global,
            None,
            "unused",
            "first line",
            AppendOpts::Plain,
            "2026-04-18T10:00:00-05:00",
            &[],
        )
        .unwrap();
        let got = svc.read_file(&Scope::Global, None, "unused").unwrap();
        let (fm, body) = frontmatter::split(&got);
        assert!(fm.is_some());
        assert_eq!(body, "\nfirst line");
    }

    #[test]
    fn service_append_timestamped_produces_entry_block() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        svc.append_file(
            &Scope::Global,
            None,
            "unused",
            "fix shipped",
            AppendOpts::Timestamped {
                timestamp: "2026-04-18 14:30",
                id: "a1b2c3d4",
                include_web_anchor: true,
            },
            "2026-04-18T14:30:00-05:00",
            &[],
        )
        .unwrap();
        let got = svc.read_file(&Scope::Global, None, "unused").unwrap();
        assert!(got.contains("<a id=\"entry-a1b2c3d4\"></a>"));
        assert!(got.contains("## 2026-04-18 14:30"));
        assert!(got.contains("^entry-a1b2c3d4"));
        assert!(got.contains("fix shipped"));
    }

    #[test]
    fn service_append_plain_adds_to_existing_body() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        svc.write_file(
            &Scope::Global,
            None,
            "unused",
            "first line\n",
            "2026-04-18T10:00:00-05:00",
            &[],
        )
        .unwrap();
        svc.append_file(
            &Scope::Global,
            None,
            "unused",
            "second line",
            AppendOpts::Plain,
            "2026-04-18T10:05:00-05:00",
            &[],
        )
        .unwrap();
        let got = svc.read_file(&Scope::Global, None, "unused").unwrap();
        let (_fm, body) = frontmatter::split(&got);
        assert_eq!(body, "first line\n\nsecond line");
    }

    #[test]
    fn service_write_then_read_roundtrips_body_with_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut svc = NotesService::new(tmp.path());
        svc.write_file(
            &Scope::Global,
            None,
            "unused",
            "hello world\n",
            "2026-04-18T10:00:00-05:00",
            &[],
        )
        .unwrap();
        let got = svc.read_file(&Scope::Global, None, "unused").unwrap();
        let (fm, body) = frontmatter::split(&got);
        assert!(fm.is_some(), "frontmatter present");
        assert_eq!(body, "hello world\n");
    }

    #[test]
    fn search_finds_file_matching_frontmatter_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_note(
            tmp.path(),
            "repos/foo/notes.md",
            "---\ntype: repo\ntags:\n- api\n---\nbody\n",
        );
        write_note(tmp.path(), "repos/bar/notes.md", "---\ntype: repo\ntags:\n- ui\n---\nbody\n");
        let hits = search_by_tags(tmp.path(), None, &["api".to_string()], false);
        assert_eq!(hits, vec![p]);
    }

    #[test]
    fn search_hierarchical_prefix_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_note(tmp.path(), "repos/foo/notes.md", "---\ntags:\n- api/tls\n---\n");
        // `--tag api` should match `api/tls`.
        let hits = search_by_tags(tmp.path(), None, &["api".to_string()], false);
        assert_eq!(hits, vec![p]);
    }

    #[test]
    fn search_exact_disables_prefix_matching() {
        let tmp = tempfile::tempdir().unwrap();
        write_note(tmp.path(), "repos/foo/notes.md", "---\ntags:\n- api/tls\n---\n");
        // With exact=true, `api` does NOT match `api/tls`.
        let hits = search_by_tags(tmp.path(), None, &["api".to_string()], true);
        assert!(hits.is_empty());
    }

    #[test]
    fn search_multi_tag_is_and() {
        let tmp = tempfile::tempdir().unwrap();
        let both = write_note(tmp.path(), "repos/a/notes.md", "---\ntags:\n- api\n- tls\n---\n");
        write_note(tmp.path(), "repos/b/notes.md", "---\ntags:\n- api\n---\n");
        let hits = search_by_tags(tmp.path(), None, &["api".to_string(), "tls".to_string()], false);
        assert_eq!(hits, vec![both]);
    }

    #[test]
    fn search_matches_inline_body_tags() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_note(
            tmp.path(),
            "repos/foo/notes.md",
            "---\ntype: repo\n---\nprose mentioning #api inline\n",
        );
        let hits = search_by_tags(tmp.path(), None, &["api".to_string()], false);
        assert_eq!(hits, vec![p]);
    }

    #[test]
    fn search_scope_filter_restricts_walk() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_hit = write_note(tmp.path(), "repos/foo/notes.md", "---\ntags:\n- api\n---\n");
        write_note(tmp.path(), "sessions/xyz/notes.md", "---\ntags:\n- api\n---\n");
        let hits = search_by_tags(tmp.path(), Some("repo"), &["api".to_string()], false);
        assert_eq!(hits, vec![repo_hit]);
    }

    #[test]
    fn search_skips_dot_folders() {
        let tmp = tempfile::tempdir().unwrap();
        // The .roux index area should never be walked.
        write_note(tmp.path(), ".roux/hidden.md", "---\ntags:\n- api\n---\n");
        let hits = search_by_tags(tmp.path(), None, &["api".to_string()], false);
        assert!(hits.is_empty());
    }

    #[test]
    fn vault_path_builds_scope_directories() {
        let v = VaultPath::new("/vault");
        assert_eq!(v.root(), Path::new("/vault"));
        assert_eq!(v.global_dir(), Path::new("/vault/global"));
        assert_eq!(v.repo_dir("phin-tech-roux"), Path::new("/vault/repos/phin-tech-roux"));
        assert_eq!(v.project_dir("marketing"), Path::new("/vault/projects/marketing"));
        assert_eq!(v.session_dir("feat--a1b2c3"), Path::new("/vault/sessions/feat--a1b2c3"));
    }

    #[test]
    fn vault_path_builds_notes_files_for_each_scope() {
        let v = VaultPath::new("/vault");
        assert_eq!(
            v.notes_file(&Scope::Global, None, "irrelevant"),
            Path::new("/vault/global/notes.md")
        );
        let repo = Scope::Repo { slug: "r".to_string(), repo_path: "/p".to_string(), remote: None };
        assert_eq!(v.notes_file(&repo, None, "irrelevant"), Path::new("/vault/repos/r/notes.md"));
        assert_eq!(
            v.notes_file(&repo, Some("api-gotchas"), "irrelevant"),
            Path::new("/vault/repos/r/api-gotchas.md")
        );
        let session = Scope::Session {
            session_id: "id".to_string(),
            repo_slug: "r".to_string(),
            project_slug: None,
            branch: "main".to_string(),
            worktree: "/w".to_string(),
        };
        assert_eq!(
            v.notes_file(&session, None, "feat--a1b2c3"),
            Path::new("/vault/sessions/feat--a1b2c3/notes.md")
        );
    }

    #[test]
    fn index_resolve_project_uses_slugified_name_and_is_frozen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut idx = NotesIndex::load(tmp.path());
        let s1 = idx.resolve_project("proj-1", "Marketing Site Revamp");
        assert_eq!(s1, "marketing-site-revamp");
        // Even if the project's name changes later, the slug stays frozen.
        let s2 = idx.resolve_project("proj-1", "Something Completely Different");
        assert_eq!(s2, "marketing-site-revamp");
    }

    #[test]
    fn index_resolve_project_adds_suffix_on_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut idx = NotesIndex::load(tmp.path());
        assert_eq!(idx.resolve_project("id-a", "Revamp"), "revamp");
        assert_eq!(idx.resolve_project("id-b", "Revamp"), "revamp-2");
    }

    #[test]
    fn index_persists_across_reload() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut idx = NotesIndex::load(tmp.path());
            idx.resolve_repo("/Users/sam/src/roux", Some("git@github.com:phin-tech/roux.git"));
        }
        let mut idx2 = NotesIndex::load(tmp.path());
        // Same repo_path must return the same slug after reload without
        // consulting the remote again.
        assert_eq!(idx2.resolve_repo("/Users/sam/src/roux", None), "phin-tech-roux");
    }

    #[test]
    fn frontmatter_ensure_writes_session_scope_fields() {
        let scope = Scope::Session {
            session_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
            repo_slug: "phin-tech-roux".to_string(),
            project_slug: Some("marketing-revamp".to_string()),
            branch: "feature/session-notes".to_string(),
            worktree: "/Users/sam/src/worktrees/session-notes".to_string(),
        };
        let got = frontmatter::ensure("", &scope, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert_eq!(map["type"].as_str(), Some("session"));
        assert_eq!(map["session_id"].as_str(), Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert_eq!(map["repo"].as_str(), Some("phin-tech-roux"));
        assert_eq!(map["project"].as_str(), Some("marketing-revamp"));
        assert_eq!(map["branch"].as_str(), Some("feature/session-notes"));
        assert_eq!(map["worktree"].as_str(), Some("/Users/sam/src/worktrees/session-notes"));
    }

    #[test]
    fn frontmatter_ensure_session_with_no_project_writes_null_project() {
        let scope = Scope::Session {
            session_id: "xyz".to_string(),
            repo_slug: "playground".to_string(),
            project_slug: None,
            branch: "main".to_string(),
            worktree: "/tmp/playground".to_string(),
        };
        let got = frontmatter::ensure("", &scope, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert!(map["project"].is_null());
    }

    #[test]
    fn frontmatter_ensure_omits_remote_when_none() {
        let scope = Scope::Repo {
            slug: "playground".to_string(),
            repo_path: "/tmp/playground".to_string(),
            remote: None,
        };
        let got = frontmatter::ensure("", &scope, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert!(!map.contains_key(serde_yaml::Value::String("remote".into())));
    }

    #[test]
    fn frontmatter_ensure_writes_project_scope_fields() {
        let scope = Scope::Project {
            slug: "marketing-revamp".to_string(),
            name: "Marketing Site Revamp".to_string(),
        };
        let got = frontmatter::ensure("", &scope, "2026-04-18T14:30:00-05:00", &[]);
        let map = parsed_fm(&got);
        assert_eq!(map["type"].as_str(), Some("project"));
        assert_eq!(map["project"].as_str(), Some("marketing-revamp"));
        assert_eq!(map["project_name"].as_str(), Some("Marketing Site Revamp"));
        let tags: Vec<&str> =
            map["tags"].as_sequence().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(tags, vec!["roux/project"]);
    }
}
