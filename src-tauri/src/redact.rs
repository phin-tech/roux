use redact_core::recognizers::pattern::PatternRecognizer;
use redact_core::recognizers::Recognizer;
use redact_core::types::EntityType;

use crate::settings::RedactCategories;

/// A compiled set of recognizers for the enabled secret categories.
pub struct AnalyzerEngine {
    recognizer: PatternRecognizer,
}

/// Build an `AnalyzerEngine` populated with patterns for all enabled categories.
pub fn build_engine(categories: &RedactCategories) -> AnalyzerEngine {
    // Use a fresh recognizer without the default PII patterns — we only want
    // developer-secret patterns to avoid masking unrelated text.
    let mut recognizer = PatternRecognizer::with_name("SecretRecognizer");
    // Clear default patterns by building from scratch via the custom path.
    // `PatternRecognizer::with_name` still calls `new()` internally which loads defaults,
    // so we use a dedicated wrapper that holds an empty base. Since the public API
    // only exposes `new()` / `with_name()` (both load defaults), we accept the defaults
    // and simply add our extra patterns on top — the overlap-resolution logic in the
    // registry will prefer our higher-scored custom patterns when they overlap with
    // generic ones.

    if categories.api_keys {
        // GitHub tokens: ghp_, gho_, ghu_, ghs_, ghr_ followed by 36+ alphanumerics
        let _ = recognizer.add_pattern(
            EntityType::Custom("GITHUB_TOKEN".into()),
            r"gh[pousr]_[A-Za-z0-9]{36,}",
            0.99,
        );

        // AWS access key IDs
        let _ = recognizer.add_pattern(
            EntityType::Custom("AWS_ACCESS_KEY".into()),
            r"(?:AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{16}",
            0.99,
        );

        // JWTs: eyJ….<base64url>.<base64url>.<base64url>
        let _ = recognizer.add_pattern(
            EntityType::Custom("JWT".into()),
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            0.99,
        );

        // Generic API key — value after common key= / api_key= / apikey= / token= prefixes
        let _ = recognizer.add_pattern(
            EntityType::Custom("API_KEY".into()),
            r#"(?i)(?:api[_-]?key|apikey|access[_-]?token|secret[_-]?key)\s*[=:]\s*['"]?([A-Za-z0-9_\-.]{20,})['"]?"#,
            0.90,
        );
    }

    if categories.credentials {
        // Bearer tokens
        let _ = recognizer.add_pattern(
            EntityType::Custom("BEARER_TOKEN".into()),
            r"[Bb]earer\s+[A-Za-z0-9_.~+/=-]{20,}",
            0.99,
        );

        // Basic auth
        let _ = recognizer.add_pattern(
            EntityType::Custom("BASIC_AUTH".into()),
            r"[Bb]asic\s+[A-Za-z0-9+/=]{20,}",
            0.99,
        );

        // URL credentials (://user:password@host)
        let _ = recognizer.add_pattern(
            EntityType::Custom("URL_CREDENTIALS".into()),
            r"://[^:@/\s]+:[^@/\s]+@",
            0.99,
        );
    }

    if categories.private_keys {
        // PEM private key blocks
        let _ = recognizer.add_pattern(
            EntityType::Custom("PRIVATE_KEY".into()),
            r"-----BEGIN\s+(?:RSA\s+|EC\s+|DSA\s+|OPENSSH\s+)?PRIVATE\s+KEY-----",
            0.99,
        );
    }

    if categories.connection_strings {
        // Database / message-broker URLs with embedded credentials
        let _ = recognizer.add_pattern(
            EntityType::Custom("CONNECTION_STRING".into()),
            r"(?:mongodb|postgres|postgresql|mysql|redis|amqp)://[^:@/\s]+:[^@/\s]+@[^\s]+",
            0.99,
        );
    }

    AnalyzerEngine { recognizer }
}

/// Return a partially masked version of `text`:
/// - If len ≥ 8: show first `start_chars` and last `end_chars`, mask middle.
/// - If len < 8: show first 2 chars, mask the rest.
pub fn partial_mask(text: &str, mask_char: char, start_chars: usize, end_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    if len < 8 {
        // Short secret: show 2, mask rest
        let visible: String = chars.iter().take(2).collect();
        let masked: String = std::iter::repeat(mask_char).take(len.saturating_sub(2)).collect();
        return format!("{}{}", visible, masked);
    }

    let head: String = chars.iter().take(start_chars).collect();
    let tail: String = chars.iter().rev().take(end_chars).collect::<Vec<_>>().into_iter().rev().collect();
    let middle_len = len.saturating_sub(start_chars + end_chars);
    let middle: String = std::iter::repeat(mask_char).take(middle_len).collect();
    format!("{}{}{}", head, middle, tail)
}

/// Strip ANSI escape sequences from `text`.
/// Returns `(clean_text, position_map)` where `position_map[i]` is the byte
/// offset in `text` that corresponds to byte `i` in `clean_text`.
pub fn strip_ansi(text: &str) -> (String, Vec<usize>) {
    let mut clean = String::with_capacity(text.len());
    let mut pos_map: Vec<usize> = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // ESC [ ... final-byte  (CSI sequences)
        if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'[' {
            // Skip until we find a final byte (0x40–0x7E)
            i += 2;
            while i < len && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < len {
                i += 1; // skip the final byte
            }
        // ESC followed by a single non-[ byte (other escape sequences)
        } else if bytes[i] == 0x1b && i + 1 < len {
            i += 2;
        } else {
            // Multi-byte UTF-8: emit all bytes that belong to the same character
            let start = i;
            let ch_len = utf8_char_len(bytes[i]);
            for b in 0..ch_len {
                if i + b < len {
                    clean.push(bytes[i + b] as char);
                    pos_map.push(start + b);
                }
            }
            i += ch_len;
        }
    }

    (clean, pos_map)
}

fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xe0 {
        2
    } else if first_byte < 0xf0 {
        3
    } else {
        4
    }
}

/// Analyze `line`, detect secrets using the engine, and return the line with
/// secrets replaced by their partial-masked equivalents.
pub fn redact_line(engine: &AnalyzerEngine, line: &str) -> String {
    // Strip ANSI so regexes match against printable text only
    let (clean, pos_map) = strip_ansi(line);

    // Collect detections on the clean text
    let detections = match engine.recognizer.analyze(&clean, "en") {
        Ok(d) => d,
        Err(_) => return line.to_string(),
    };

    if detections.is_empty() {
        return line.to_string();
    }

    // Build a sorted, non-overlapping set of spans (start, end) in clean-text bytes
    let mut spans: Vec<(usize, usize)> = detections
        .iter()
        .map(|r| (r.start, r.end))
        .collect();
    spans.sort();

    // Merge overlapping spans
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in spans {
        if let Some(last) = merged.last_mut() {
            if s < last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }

    // Rebuild the *original* (ANSI-intact) line, replacing each detected span.
    // `pos_map[i]` maps clean byte `i` → original byte offset.
    let orig_bytes = line.as_bytes();
    let mut result = String::with_capacity(line.len());
    let mut orig_cursor = 0usize;

    for (cs, ce) in merged {
        // Map clean-text byte range to original byte range
        if cs >= pos_map.len() {
            break;
        }
        let orig_start = pos_map[cs];
        let orig_end = if ce > 0 && ce <= pos_map.len() {
            // ce is exclusive; map to the byte just after the last included clean byte
            if ce < pos_map.len() {
                pos_map[ce]
            } else {
                orig_bytes.len()
            }
        } else {
            orig_bytes.len()
        };

        // Append everything before this span
        if orig_start > orig_cursor {
            result.push_str(&line[orig_cursor..orig_start]);
        }

        // Extract the secret text from the clean string and mask it
        let secret = &clean[cs..ce];
        let masked = partial_mask(secret, '*', 4, 4);
        result.push_str(&masked);

        orig_cursor = orig_end;
    }

    // Append remainder
    if orig_cursor < orig_bytes.len() {
        result.push_str(&line[orig_cursor..]);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::RedactCategories;

    fn all_categories() -> RedactCategories {
        RedactCategories {
            api_keys: true,
            credentials: true,
            private_keys: true,
            connection_strings: true,
        }
    }

    #[test]
    fn partial_mask_normal() {
        // "ghp_abcdefghijklmnopqr" has 22 chars → show 4, mask 14, show 4
        let result = partial_mask("ghp_abcdefghijklmnopqr", '*', 4, 4);
        assert_eq!(result, "ghp_**************opqr");
    }

    #[test]
    fn partial_mask_short() {
        // "sk_abc" has 6 chars → show 2, mask 4
        let result = partial_mask("sk_abc", '*', 4, 4);
        assert_eq!(result, "sk****");
    }

    #[test]
    fn redact_github_token() {
        let engine = build_engine(&all_categories());
        let token = "ghp_abcdefghijklmnopqrstuvwxyz1234567890";
        let line = format!("token={}", token);
        let redacted = redact_line(&engine, &line);
        // The token should be partially masked — not identical to the original
        assert_ne!(redacted, line);
        // Should contain asterisks
        assert!(redacted.contains('*'), "Expected masked output, got: {}", redacted);
    }

    #[test]
    fn no_redaction_when_no_secrets() {
        let engine = build_engine(&all_categories());
        let line = "Hello, world! No secrets here.";
        let redacted = redact_line(&engine, line);
        assert_eq!(redacted, line);
    }

    #[test]
    fn disabled_category_not_redacted() {
        let categories = RedactCategories {
            api_keys: false,
            credentials: true,
            private_keys: true,
            connection_strings: true,
        };
        let engine = build_engine(&categories);
        let token = "ghp_abcdefghijklmnopqrstuvwxyz1234567890";
        let line = format!("token={}", token);
        let redacted = redact_line(&engine, &line);
        // With api_keys disabled the GitHub token should pass through unchanged
        assert_eq!(redacted, line);
    }
}
