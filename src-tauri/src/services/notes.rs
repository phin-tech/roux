//! Multi-scoped notes service.
//!
//! **Experimental.** Vault layout, frontmatter schema, CLI surface, env var
//! names, and Tauri command signatures exposed from this module are all
//! subject to change. See `docs/superpowers/specs/2026-04-18-notes-expansion-design.md`
//! for the full design and stability guarantees (or lack thereof).

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum NotesError {
    #[error("invalid topic name")]
    InvalidTopic,
}

pub(crate) mod topic {
    use super::NotesError;

    pub(crate) fn slugify(name: &str) -> Result<String, NotesError> {
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
}
