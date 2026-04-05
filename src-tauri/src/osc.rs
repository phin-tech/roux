use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub status: String,
    pub model: Option<String>,
    pub cost: Option<f64>,
}

/// Scans a byte buffer for OSC title-set sequences (\x1b]0;...\x07 or \x1b]0;...\x1b\\)
/// and extracts Claude Code status information from the title string.
pub fn parse_osc_status(buf: &[u8]) -> Option<SessionStatus> {
    let text = String::from_utf8_lossy(buf);
    let mut last_title: Option<&str> = None;

    // Find all OSC sequences and keep the last one
    let mut search = text.as_ref();
    while let Some(start) = search.find("\x1b]") {
        let after_osc = &search[start + 2..];
        // Find BEL terminator (\x07) or ST terminator (\x1b\\, 2 bytes)
        let (end_pos, terminator_len) = if let Some(pos) = after_osc.find('\x07') {
            (pos, 1)
        } else if let Some(pos) = after_osc.find("\x1b\\") {
            (pos, 2)
        } else {
            break;
        };
        let payload = &after_osc[..end_pos];
        // OSC 0 or OSC 2 set window title
        if let Some(title) = payload.strip_prefix("0;").or_else(|| payload.strip_prefix("2;")) {
            last_title = Some(title);
        }
        search = &after_osc[end_pos + terminator_len..];
    }

    let title = last_title?;

    // Parse Claude Code title format
    // Typical: "Thinking | ~/project | personal | Opus 4.6 (1M) | 2m | $0.16 | 5%"
    // Or: "Generating | ~/project | ..."
    // Or: "Idle | ~/project | ..."
    let parts: Vec<&str> = title.split(" | ").collect();
    if parts.is_empty() {
        return None;
    }

    let status_str = parts[0].trim().to_lowercase();
    let status = match status_str.as_str() {
        s if s.contains("think") => "thinking",
        s if s.contains("generat") => "generating",
        s if s.contains("idle") => "idle",
        _ => return None,
    };

    let mut model: Option<String> = None;
    let mut cost: Option<f64> = None;

    for part in &parts[1..] {
        let trimmed = part.trim();
        if trimmed.starts_with('$') {
            if let Ok(c) = trimmed[1..].parse::<f64>() {
                cost = Some(c);
            }
        } else if trimmed.contains("Opus")
            || trimmed.contains("Sonnet")
            || trimmed.contains("Haiku")
        {
            model = Some(trimmed.to_string());
        }
    }

    Some(SessionStatus {
        status: status.to_string(),
        model,
        cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thinking_status() {
        let buf = b"\x1b]0;Thinking | ~/project | personal | Opus 4.6 (1M) | 2m | $0.16 | 5%\x07";
        let result = parse_osc_status(buf).unwrap();
        assert_eq!(result.status, "thinking");
        assert_eq!(result.model, Some("Opus 4.6 (1M)".to_string()));
        assert_eq!(result.cost, Some(0.16));
    }

    #[test]
    fn test_parse_idle_status() {
        let buf = b"\x1b]0;Idle | ~/project | personal | Sonnet 4.6 | 0m | $0.00 | 0%\x07";
        let result = parse_osc_status(buf).unwrap();
        assert_eq!(result.status, "idle");
        assert_eq!(result.model, Some("Sonnet 4.6".to_string()));
        assert_eq!(result.cost, Some(0.0));
    }

    #[test]
    fn test_no_osc_returns_none() {
        let buf = b"Hello world, no OSC here";
        assert!(parse_osc_status(buf).is_none());
    }

    #[test]
    fn test_parse_generating_with_st_terminator() {
        let buf = b"\x1b]0;Generating | ~/project | Opus 4.6 (1M) | $1.23\x1b\\";
        let result = parse_osc_status(buf).unwrap();
        assert_eq!(result.status, "generating");
        assert_eq!(result.cost, Some(1.23));
    }
}
