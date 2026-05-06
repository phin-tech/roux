use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexNotificationPreview {
    pub config_path: PathBuf,
    pub configured: bool,
    pub current_value: Option<String>,
    pub next_content: String,
}

pub(crate) fn codex_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex").join("config.toml"))
}

pub(crate) fn preview_codex_notification_config_at(
    config_path: &Path,
) -> Result<CodexNotificationPreview, String> {
    let existing = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("failed to read Codex config: {e}")),
    };
    let current_value = codex_notification_condition(&existing);
    let configured = current_value.as_deref() == Some("always");
    let next_content = ensure_codex_notification_condition(&existing);
    Ok(CodexNotificationPreview {
        config_path: config_path.to_path_buf(),
        configured,
        current_value,
        next_content,
    })
}

pub(crate) fn configure_codex_notification_config_at(config_path: &Path) -> Result<(), String> {
    let preview = preview_codex_notification_config_at(config_path)?;
    if preview.configured {
        return Ok(());
    }
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create Codex config directory: {e}"))?;
    }
    fs::write(config_path, preview.next_content)
        .map_err(|e| format!("failed to write Codex config: {e}"))
}

fn codex_notification_condition(content: &str) -> Option<String> {
    let mut in_tui = false;
    for line in content.lines() {
        if let Some(section) = parse_table_header(line) {
            in_tui = section == "tui";
            continue;
        }
        if !in_tui {
            continue;
        }
        let Some((key, value)) = parse_key_value(line) else {
            continue;
        };
        if key == "notification_condition" {
            return Some(unquote_toml_string(value).to_string());
        }
    }
    None
}

fn ensure_codex_notification_condition(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    if normalized.trim().is_empty() {
        return "[tui]\nnotification_condition = \"always\"\n".to_string();
    }

    let lines: Vec<&str> = normalized.lines().collect();
    let mut out = Vec::with_capacity(lines.len() + 3);
    let mut in_tui = false;
    let mut saw_tui = false;
    let mut wrote_condition = false;

    for line in lines {
        if let Some(section) = parse_table_header(line) {
            if in_tui && !wrote_condition {
                out.push("notification_condition = \"always\"".to_string());
                wrote_condition = true;
            }
            in_tui = section == "tui";
            saw_tui |= in_tui;
        }

        if in_tui {
            if let Some((key, _value)) = parse_key_value(line) {
                if key == "notification_condition" {
                    out.push(rewrite_notification_condition_line(line));
                    wrote_condition = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }

    if saw_tui && !wrote_condition {
        out.push("notification_condition = \"always\"".to_string());
    } else if !saw_tui {
        if !out.last().map(|line| line.trim().is_empty()).unwrap_or(true) {
            out.push(String::new());
        }
        out.push("[tui]".to_string());
        out.push("notification_condition = \"always\"".to_string());
    }

    let mut next = out.join("\n");
    next.push('\n');
    next
}

fn rewrite_notification_condition_line(line: &str) -> String {
    let leading: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let comment =
        split_value_comment(line).1.map(|comment| format!(" {comment}")).unwrap_or_default();
    format!("{leading}notification_condition = \"always\"{comment}")
}

fn parse_table_header(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    if trimmed.starts_with("[[") {
        return None;
    }
    let inner = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
    (!inner.is_empty()).then_some(inner)
}

fn parse_key_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let (before_comment, _) = split_value_comment(trimmed);
    let (key, value) = before_comment.split_once('=')?;
    Some((key.trim(), value.trim()))
}

fn split_value_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_string = false;
    let mut escaped = false;
    for (idx, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '#' if !in_string => return (line[..idx].trim_end(), Some(line[idx..].trim())),
            _ => {}
        }
    }
    (line.trim_end(), None)
}

fn unquote_toml_string(value: &str) -> &str {
    value.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_missing_config_creates_tui_section() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");

        let preview = preview_codex_notification_config_at(&path).unwrap();

        assert!(!preview.configured);
        assert_eq!(preview.current_value, None);
        assert_eq!(preview.next_content, "[tui]\nnotification_condition = \"always\"\n");
    }

    #[test]
    fn preview_preserves_existing_toml_and_adds_tui_section() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "# Codex\nmodel = \"gpt-5\"\n").unwrap();

        let preview = preview_codex_notification_config_at(&path).unwrap();

        assert_eq!(
            preview.next_content,
            "# Codex\nmodel = \"gpt-5\"\n\n[tui]\nnotification_condition = \"always\"\n",
        );
    }

    #[test]
    fn preview_adds_condition_to_existing_tui_section() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "[tui]\ntheme = \"dark\"\n[model]\nname = \"gpt-5\"\n").unwrap();

        let preview = preview_codex_notification_config_at(&path).unwrap();

        assert_eq!(
            preview.next_content,
            "[tui]\ntheme = \"dark\"\nnotification_condition = \"always\"\n[model]\nname = \"gpt-5\"\n",
        );
    }

    #[test]
    fn preview_replaces_existing_condition_and_preserves_comment() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "[tui]\nnotification_condition = \"never\" # old\n").unwrap();

        let preview = preview_codex_notification_config_at(&path).unwrap();

        assert!(!preview.configured);
        assert_eq!(preview.current_value.as_deref(), Some("never"));
        assert_eq!(preview.next_content, "[tui]\nnotification_condition = \"always\" # old\n",);
    }

    #[test]
    fn preview_reports_already_configured() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "[tui]\nnotification_condition = \"always\"\n").unwrap();

        let preview = preview_codex_notification_config_at(&path).unwrap();

        assert!(preview.configured);
        assert_eq!(preview.current_value.as_deref(), Some("always"));
        assert_eq!(preview.next_content, "[tui]\nnotification_condition = \"always\"\n");
    }

    #[test]
    fn configure_writes_preview_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".codex").join("config.toml");

        configure_codex_notification_config_at(&path).unwrap();

        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "[tui]\nnotification_condition = \"always\"\n",
        );
    }
}
