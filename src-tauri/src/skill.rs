//! Ship a Claude Code skill so agents running inside Roux panes can discover
//! the control CLI and drive the host app without per-project setup.
//!
//! The skill lives at `~/.claude/skills/roux/SKILL.md`. An in-file comment
//! carries an integer version so Roux can overwrite stale installs without
//! clobbering user edits to unrelated skills.

use std::fs;
use std::path::{Path, PathBuf};

/// Bumped any time [`SKILL_CONTENT`] changes. Must match the
/// `roux-skill-version:` marker inside the content.
pub const SKILL_VERSION: u32 = 1;

pub const SKILL_CONTENT: &str = include_str!("skill/SKILL.md");

const VERSION_MARKER: &str = "roux-skill-version:";

pub fn skill_install_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("skills").join("roux").join("SKILL.md"))
}

pub fn skill_is_installed() -> bool {
    match installed_version() {
        Some(v) => v >= SKILL_VERSION,
        None => false,
    }
}

pub fn installed_version() -> Option<u32> {
    let path = skill_install_path()?;
    let content = fs::read_to_string(&path).ok()?;
    parse_version(&content)
}

/// Parse the `roux-skill-version:` marker out of skill content.
/// Accepts the marker anywhere on a line (typically inside a comment).
pub fn parse_version(content: &str) -> Option<u32> {
    for line in content.lines() {
        if let Some(idx) = line.find(VERSION_MARKER) {
            let rest = &line[idx + VERSION_MARKER.len()..];
            // Take the first run of digits after the marker.
            let digits: String = rest.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
        }
    }
    None
}

/// Install (or upgrade) the skill. Returns the target path.
///
/// Writes iff the target is missing or its version is older than
/// [`SKILL_VERSION`]. Atomic-ish via temp-file + rename.
pub fn install_skill() -> Result<PathBuf, String> {
    let target = skill_install_path().ok_or("Could not determine home directory")?;
    install_skill_at(&target, SKILL_CONTENT, SKILL_VERSION)
}

fn install_skill_at(target: &Path, content: &str, version: u32) -> Result<PathBuf, String> {
    if let Some(existing) = fs::read_to_string(target).ok().as_deref().and_then(parse_version) {
        if existing >= version {
            return Ok(target.to_path_buf());
        }
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    let tmp = target.with_extension("md.tmp");
    fs::write(&tmp, content).map_err(|e| format!("Failed to write skill tmp: {}", e))?;
    fs::rename(&tmp, target).map_err(|e| format!("Failed to install skill: {}", e))?;
    Ok(target.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_content_contains_matching_version_marker() {
        let parsed = parse_version(SKILL_CONTENT)
            .expect("bundled SKILL.md must contain a roux-skill-version: marker");
        assert_eq!(
            parsed, SKILL_VERSION,
            "SKILL_VERSION ({}) and in-file marker ({}) drifted",
            SKILL_VERSION, parsed
        );
    }

    #[test]
    fn parse_version_handles_comment_styles() {
        assert_eq!(parse_version("<!-- roux-skill-version: 3 -->"), Some(3));
        assert_eq!(parse_version("# roux-skill-version:42"), Some(42));
        assert_eq!(parse_version("stuff\n<!-- roux-skill-version: 7 -->\nmore"), Some(7));
    }

    #[test]
    fn parse_version_returns_none_when_absent_or_malformed() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("no marker here"), None);
        assert_eq!(parse_version("roux-skill-version: abc"), None);
    }

    #[test]
    fn install_writes_when_target_missing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("skills").join("roux").join("SKILL.md");
        let content = "<!-- roux-skill-version: 2 -->\nhello";
        install_skill_at(&target, content, 2).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), content);
    }

    #[test]
    fn install_skips_when_existing_version_is_equal_or_newer() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("SKILL.md");
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(&target, "<!-- roux-skill-version: 5 -->\nolder bundled ignored").unwrap();

        install_skill_at(&target, "<!-- roux-skill-version: 2 -->\nnew content", 2).unwrap();
        let after = fs::read_to_string(&target).unwrap();
        assert!(after.contains("older bundled ignored"), "should not have overwritten newer");
    }

    #[test]
    fn install_overwrites_stale_versions() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("SKILL.md");
        fs::write(&target, "<!-- roux-skill-version: 1 -->\nold").unwrap();
        install_skill_at(&target, "<!-- roux-skill-version: 2 -->\nnew", 2).unwrap();
        assert!(fs::read_to_string(&target).unwrap().contains("new"));
    }

    #[test]
    fn install_overwrites_unparseable_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("SKILL.md");
        fs::write(&target, "garbage with no marker").unwrap();
        install_skill_at(&target, "<!-- roux-skill-version: 1 -->\nok", 1).unwrap();
        assert!(fs::read_to_string(&target).unwrap().contains("ok"));
    }
}
