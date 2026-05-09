//! Topic glob matching for bus subscriptions.
//!
//! MQTT-style segment-aware globbing. Topics are dot-separated segments
//! (`repo-a.build.completed`); patterns may use literal segments, `*` for
//! exactly one segment, and `**` for zero or more segments.
//!
//! Examples:
//! - `repo-a.*` matches `repo-a.build`, NOT `repo-a.build.completed`
//! - `repo-a.**` matches `repo-a`, `repo-a.build`, `repo-a.build.completed`
//! - `*.completed` matches `build.completed`, NOT `repo-a.build.completed`
//! - `**.completed` matches both
//!
//! Pattern segments are restricted to the same alphabet as topics
//! (lowercase letters, digits, hyphens) plus the wildcards. Mixed tokens
//! (e.g. `foo*`) are rejected at validation time so we don't have to
//! reason about partial-segment globbing.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    Empty,
    EmptySegment,
    InvalidSegment(String),
    TooLong,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternError::Empty => f.write_str("topic pattern is empty"),
            PatternError::EmptySegment => f.write_str(
                "topic pattern has an empty segment (consecutive dots or leading/trailing dot)",
            ),
            PatternError::InvalidSegment(s) => write!(
                f,
                "topic pattern segment '{s}' is invalid; must be `*`, `**`, or [a-z0-9-]+",
            ),
            PatternError::TooLong => f.write_str("topic pattern is too long (max 256 chars)"),
        }
    }
}

impl std::error::Error for PatternError {}

const MAX_PATTERN_LEN: usize = 256;

/// Validate a topic pattern. Returns the canonical form (currently a
/// no-op pass-through; reserved for future case-folding). Patterns are
/// the only place wildcards are allowed; concrete topics should pass
/// `validate_topic` instead.
pub fn validate_topic_pattern(pattern: &str) -> Result<String, PatternError> {
    if pattern.is_empty() {
        return Err(PatternError::Empty);
    }
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(PatternError::TooLong);
    }
    for segment in pattern.split('.') {
        if segment.is_empty() {
            return Err(PatternError::EmptySegment);
        }
        if segment == "*" || segment == "**" {
            continue;
        }
        if !is_literal_segment(segment) {
            return Err(PatternError::InvalidSegment(segment.to_string()));
        }
    }
    Ok(pattern.to_string())
}

/// Validate a concrete (no-wildcard) topic. Used by `bus publish` to
/// reject malformed topic strings before they hit the store.
pub fn validate_topic(topic: &str) -> Result<String, PatternError> {
    if topic.is_empty() {
        return Err(PatternError::Empty);
    }
    if topic.len() > MAX_PATTERN_LEN {
        return Err(PatternError::TooLong);
    }
    for segment in topic.split('.') {
        if segment.is_empty() {
            return Err(PatternError::EmptySegment);
        }
        if !is_literal_segment(segment) {
            return Err(PatternError::InvalidSegment(segment.to_string()));
        }
    }
    Ok(topic.to_string())
}

fn is_literal_segment(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// True when `topic` matches `pattern`. Both are dot-separated. `pattern`
/// may use `*` (exactly one segment) and `**` (zero or more segments).
///
/// Caller is responsible for passing a validated pattern; an unvalidated
/// pattern with weird tokens like `foo*bar` will be treated as a literal
/// segment and won't match anything that doesn't equal it byte-for-byte.
pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    if pattern.is_empty() || topic.is_empty() {
        return false;
    }
    let pat: Vec<&str> = pattern.split('.').collect();
    let top: Vec<&str> = topic.split('.').collect();
    matches_segments(&pat, &top)
}

fn matches_segments(pattern: &[&str], topic: &[&str]) -> bool {
    match (pattern.first(), topic.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(p), _) if *p == "**" => {
            // `**` matches zero or more segments. Try every length.
            let rest_pat = &pattern[1..];
            // zero segments: drop the `**` and continue against the same topic
            if matches_segments(rest_pat, topic) {
                return true;
            }
            // one or more segments: consume one topic segment, retry
            for i in 1..=topic.len() {
                if matches_segments(rest_pat, &topic[i..]) {
                    return true;
                }
            }
            false
        }
        (Some(_), None) => false,
        (Some(p), Some(t)) => {
            let head_ok = *p == "*" || *p == *t;
            head_ok && matches_segments(&pattern[1..], &topic[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── topic_matches ───────────────────────────────────────────────

    #[test]
    fn exact_match_succeeds() {
        assert!(topic_matches("repo-a.build", "repo-a.build"));
    }

    #[test]
    fn exact_pattern_rejects_extra_segments() {
        assert!(!topic_matches("repo-a.build", "repo-a.build.completed"));
        assert!(!topic_matches("repo-a.build", "repo-a"));
    }

    #[test]
    fn single_star_consumes_exactly_one_segment() {
        assert!(topic_matches("repo-a.*", "repo-a.build"));
        assert!(!topic_matches("repo-a.*", "repo-a.build.completed"));
        assert!(!topic_matches("repo-a.*", "repo-a"));
    }

    #[test]
    fn single_star_at_head_matches_one_leading_segment() {
        assert!(topic_matches("*.completed", "build.completed"));
        assert!(!topic_matches("*.completed", "repo-a.build.completed"));
    }

    #[test]
    fn double_star_at_tail_matches_any_suffix_including_zero() {
        assert!(topic_matches("repo-a.**", "repo-a"));
        assert!(topic_matches("repo-a.**", "repo-a.build"));
        assert!(topic_matches("repo-a.**", "repo-a.build.completed"));
        assert!(!topic_matches("repo-a.**", "repo-b.build"));
    }

    #[test]
    fn double_star_at_head_matches_any_prefix() {
        assert!(topic_matches("**.completed", "completed"));
        assert!(topic_matches("**.completed", "build.completed"));
        assert!(topic_matches("**.completed", "repo-a.build.completed"));
        assert!(!topic_matches("**.completed", "build.failed"));
    }

    #[test]
    fn double_star_in_middle_matches_anything_between() {
        assert!(topic_matches("repo-a.**.completed", "repo-a.completed"));
        assert!(topic_matches("repo-a.**.completed", "repo-a.build.completed"));
        assert!(topic_matches("repo-a.**.completed", "repo-a.x.y.z.completed"));
        assert!(!topic_matches("repo-a.**.completed", "repo-a.x.y.z.failed"));
    }

    #[test]
    fn standalone_double_star_matches_anything() {
        assert!(topic_matches("**", "x"));
        assert!(topic_matches("**", "x.y.z"));
    }

    #[test]
    fn standalone_single_star_matches_only_one_segment() {
        assert!(topic_matches("*", "x"));
        assert!(!topic_matches("*", "x.y"));
    }

    #[test]
    fn mismatched_literal_does_not_match() {
        assert!(!topic_matches("repo-a.build", "repo-b.build"));
    }

    #[test]
    fn empty_inputs_never_match() {
        assert!(!topic_matches("", ""));
        assert!(!topic_matches("", "build"));
        assert!(!topic_matches("build", ""));
    }

    #[test]
    fn star_does_not_match_empty_segment() {
        // there is no concept of an empty topic segment; topic.split('.')
        // never yields one for valid topics.
        assert!(!topic_matches("repo-a.*.completed", "repo-a.completed"));
    }

    #[test]
    fn multiple_single_stars_must_align() {
        assert!(topic_matches("*.*.completed", "repo-a.build.completed"));
        assert!(!topic_matches("*.*.completed", "repo-a.completed"));
        assert!(!topic_matches("*.*.completed", "a.b.c.completed"));
    }

    // ── validate_topic_pattern ──────────────────────────────────────

    #[test]
    fn validate_pattern_accepts_literal() {
        assert_eq!(
            validate_topic_pattern("repo-a.build.completed").as_deref(),
            Ok("repo-a.build.completed"),
        );
    }

    #[test]
    fn validate_pattern_accepts_wildcards() {
        assert!(validate_topic_pattern("repo-a.*").is_ok());
        assert!(validate_topic_pattern("repo-a.**").is_ok());
        assert!(validate_topic_pattern("**.completed").is_ok());
        assert!(validate_topic_pattern("*").is_ok());
        assert!(validate_topic_pattern("**").is_ok());
    }

    #[test]
    fn validate_pattern_rejects_empty() {
        assert_eq!(validate_topic_pattern(""), Err(PatternError::Empty));
    }

    #[test]
    fn validate_pattern_rejects_empty_segments() {
        assert_eq!(
            validate_topic_pattern("repo-a..build"),
            Err(PatternError::EmptySegment),
        );
        assert_eq!(
            validate_topic_pattern(".repo-a"),
            Err(PatternError::EmptySegment),
        );
        assert_eq!(
            validate_topic_pattern("repo-a."),
            Err(PatternError::EmptySegment),
        );
    }

    #[test]
    fn validate_pattern_rejects_mixed_tokens() {
        // partial-segment globbing not supported: `foo*` is neither a
        // valid literal nor a wildcard token.
        assert!(matches!(
            validate_topic_pattern("repo-a.foo*"),
            Err(PatternError::InvalidSegment(_)),
        ));
        assert!(matches!(
            validate_topic_pattern("**foo"),
            Err(PatternError::InvalidSegment(_)),
        ));
    }

    #[test]
    fn validate_pattern_rejects_disallowed_chars() {
        for bad in ["repo_a", "repo a.build", "REPO-A.build", "repo-a/build"] {
            assert!(
                matches!(validate_topic_pattern(bad), Err(PatternError::InvalidSegment(_))),
                "expected reject for {bad:?}",
            );
        }
    }

    #[test]
    fn validate_pattern_rejects_too_long() {
        let too_long = "a".repeat(MAX_PATTERN_LEN + 1);
        assert_eq!(validate_topic_pattern(&too_long), Err(PatternError::TooLong));
    }

    // ── validate_topic (no wildcards allowed) ───────────────────────

    #[test]
    fn validate_topic_rejects_wildcards() {
        assert!(matches!(
            validate_topic("repo-a.*"),
            Err(PatternError::InvalidSegment(_)),
        ));
        assert!(matches!(
            validate_topic("**"),
            Err(PatternError::InvalidSegment(_)),
        ));
    }

    #[test]
    fn validate_topic_accepts_literal() {
        assert_eq!(
            validate_topic("repo-a.build.completed").as_deref(),
            Ok("repo-a.build.completed"),
        );
    }
}
