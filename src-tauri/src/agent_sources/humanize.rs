//! Builds human-readable attention notification titles and bodies from
//! hook payloads. Ported from the previous `status_watcher` module.
//!
//! Claude Code's PreToolUse / Notification payload is `{ tool_name,
//! tool_input }` where `tool_input` shape varies per tool. This
//! formatter produces plain English: the title becomes a short verb
//! phrase (`"Run command"`, `"Edit file"`) and the body becomes the
//! single most informative field for that tool (the command, file
//! path, URL, etc.) — never raw JSON.

use serde_json::Value;

/// Pick a "which workspace is this from?" label for the notification
/// subtitle. Prefers the user-assigned Roux session name; falls back to
/// the cwd basename so external-claude invocations without a matching
/// session still get a project hint instead of nothing.
pub fn session_label(session_name: Option<&str>, cwd: &str) -> Option<String> {
    if let Some(name) = session_name.filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }
    let trimmed = cwd.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let basename = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if basename.is_empty() {
        None
    } else {
        Some(basename.to_string())
    }
}

pub fn humanize_attention(
    tool_name: Option<&str>,
    tool_input: Option<&Value>,
    message: Option<&str>,
) -> (String, Option<String>) {
    fn s<'a>(input: Option<&'a Value>, key: &str) -> Option<&'a str> {
        input.and_then(|v| v.get(key)).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
    }
    fn truncate(s: &str, max: usize) -> String {
        if s.chars().count() <= max {
            return s.to_string();
        }
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }

    let Some(tool) = tool_name else {
        let title = message.unwrap_or("Permission requested").to_string();
        return (title, None);
    };

    let input = tool_input;
    match tool {
        "Bash" => {
            let body = s(input, "command").map(|c| truncate(c, 200));
            ("Run command".to_string(), body)
        }
        "Read" => ("Read file".to_string(), s(input, "file_path").map(|p| p.to_string())),
        "Write" => ("Write file".to_string(), s(input, "file_path").map(|p| p.to_string())),
        "Edit" | "MultiEdit" => {
            ("Edit file".to_string(), s(input, "file_path").map(|p| p.to_string()))
        }
        "Glob" => {
            let pattern = s(input, "pattern").unwrap_or("").to_string();
            let body = match s(input, "path") {
                Some(p) => Some(format!("{} in {}", pattern, p)),
                None if !pattern.is_empty() => Some(pattern),
                None => None,
            };
            ("Find files".to_string(), body)
        }
        "Grep" => {
            let pattern = s(input, "pattern").unwrap_or("").to_string();
            let body = match s(input, "path") {
                Some(p) => Some(format!("{} in {}", pattern, p)),
                None if !pattern.is_empty() => Some(pattern),
                None => None,
            };
            ("Search files".to_string(), body)
        }
        "WebFetch" => ("Fetch URL".to_string(), s(input, "url").map(|u| u.to_string())),
        "WebSearch" => ("Web search".to_string(), s(input, "query").map(|q| q.to_string())),
        "Task" => {
            let body =
                s(input, "description").or_else(|| s(input, "prompt")).map(|t| truncate(t, 200));
            ("Run task".to_string(), body)
        }
        "TodoWrite" => ("Update todos".to_string(), None),
        "NotebookEdit" => {
            ("Edit notebook".to_string(), s(input, "notebook_path").map(|p| p.to_string()))
        }
        other => {
            let body = input.and_then(|v| v.as_object()).and_then(|obj| {
                for key in [
                    "command",
                    "file_path",
                    "path",
                    "url",
                    "query",
                    "pattern",
                    "description",
                    "prompt",
                ] {
                    if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                        if !val.is_empty() {
                            return Some(truncate(val, 200));
                        }
                    }
                }
                None
            });
            (other.to_string(), body.or_else(|| message.map(|m| m.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_label_prefers_session_name() {
        assert_eq!(session_label(Some("auth-rewrite"), "/repo/x"), Some("auth-rewrite".into()));
    }

    #[test]
    fn session_label_falls_back_to_cwd_basename_when_no_session() {
        assert_eq!(session_label(None, "/Users/me/src/roux"), Some("roux".into()));
    }

    #[test]
    fn session_label_handles_trailing_slash() {
        assert_eq!(session_label(None, "/Users/me/src/roux/"), Some("roux".into()));
    }

    #[test]
    fn session_label_treats_empty_session_name_as_missing() {
        assert_eq!(session_label(Some(""), "/repo/proj"), Some("proj".into()));
    }

    #[test]
    fn session_label_returns_none_for_empty_cwd_and_no_name() {
        assert_eq!(session_label(None, ""), None);
    }

    #[test]
    fn humanize_bash_uses_command_string() {
        let input = json!({ "command": "echo hello", "description": "say hi" });
        let (title, body) = humanize_attention(Some("Bash"), Some(&input), None);
        assert_eq!(title, "Run command");
        assert_eq!(body.as_deref(), Some("echo hello"));
    }

    #[test]
    fn humanize_bash_truncates_long_commands_at_char_boundary() {
        let cmd = "x".repeat(500);
        let input = json!({ "command": cmd });
        let (_, body) = humanize_attention(Some("Bash"), Some(&input), None);
        let body = body.unwrap();
        assert!(body.ends_with('…'));
        assert_eq!(body.chars().count(), 201);
    }

    #[test]
    fn humanize_edit_uses_file_path() {
        let input =
            json!({ "file_path": "/repo/src/main.rs", "old_string": "a", "new_string": "b" });
        let (title, body) = humanize_attention(Some("Edit"), Some(&input), None);
        assert_eq!(title, "Edit file");
        assert_eq!(body.as_deref(), Some("/repo/src/main.rs"));
    }

    #[test]
    fn humanize_grep_combines_pattern_and_path() {
        let input = json!({ "pattern": "TODO", "path": "src/" });
        let (title, body) = humanize_attention(Some("Grep"), Some(&input), None);
        assert_eq!(title, "Search files");
        assert_eq!(body.as_deref(), Some("TODO in src/"));
    }

    #[test]
    fn humanize_unknown_tool_picks_known_field_not_json() {
        let input = json!({ "url": "https://example.com", "extra": 1 });
        let (title, body) = humanize_attention(Some("MyCustomTool"), Some(&input), None);
        assert_eq!(title, "MyCustomTool");
        assert_eq!(body.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn humanize_unknown_tool_with_no_recognizable_field_falls_back_to_message() {
        let input = json!({ "weird_field": "x" });
        let (title, body) = humanize_attention(Some("MyCustomTool"), Some(&input), Some("explain"));
        assert_eq!(title, "MyCustomTool");
        assert_eq!(body.as_deref(), Some("explain"));
    }

    #[test]
    fn humanize_no_tool_falls_back_to_message() {
        let (title, body) = humanize_attention(None, None, Some("Permission needed for X"));
        assert_eq!(title, "Permission needed for X");
        assert_eq!(body, None);
    }

    #[test]
    fn humanize_body_never_contains_raw_json_braces() {
        let cases = vec![
            ("Bash", json!({ "command": "ls" })),
            ("Read", json!({ "file_path": "/x" })),
            ("Edit", json!({ "file_path": "/x" })),
            ("Glob", json!({ "pattern": "*.rs" })),
            ("Grep", json!({ "pattern": "fn", "path": "src/" })),
            ("WebFetch", json!({ "url": "https://x" })),
            ("Task", json!({ "description": "do stuff" })),
        ];
        for (tool, input) in cases {
            let (_, body) = humanize_attention(Some(tool), Some(&input), None);
            let body = body.unwrap_or_default();
            assert!(!body.starts_with('{'), "tool {} produced JSON-looking body: {}", tool, body);
        }
    }
}
