# Secret Redaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically detect and partially mask secrets (API keys, tokens, credentials, private keys, connection strings) in terminal output, controlled by a settings toggle with per-category granularity.

**Architecture:** A new `redact` module in the Rust backend uses `redact-core` with custom `PatternRecognizer` patterns for developer-focused secrets. The `spawn_reader()` function in `pty.rs` is modified to optionally line-buffer and redact output before sending to the flusher. Redaction config is passed as shared atomics so setting changes take effect on active PTY sessions without restart.

**Tech Stack:** `redact-core` (Apache 2.0), Rust regex patterns, `AnonymizationStrategy::Mask` with partial reveal

---

### Task 1: Add `redact-core` dependency

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add redact-core to Cargo.toml**

```bash
cd src-tauri && cargo add redact-core
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: Compiles with new dependency

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: add redact-core dependency for secret redaction"
```

---

### Task 2: Add redaction settings to Rust and TypeScript

**Files:**
- Modify: `src-tauri/src/settings.rs`
- Modify: `src/lib/types.ts`

- [ ] **Step 1: Add RedactCategories struct and fields to RouxSettings**

In `src-tauri/src/settings.rs`, add after the `default_group_by` function:

```rust
fn default_redact_categories() -> RedactCategories {
    RedactCategories::default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactCategories {
    pub api_keys: bool,
    pub credentials: bool,
    pub private_keys: bool,
    pub connection_strings: bool,
}

impl Default for RedactCategories {
    fn default() -> Self {
        Self {
            api_keys: true,
            credentials: true,
            private_keys: true,
            connection_strings: true,
        }
    }
}
```

Add to `RouxSettings` struct after `group_by`:

```rust
    #[serde(default)]
    pub redact_secrets: bool,
    #[serde(default = "default_redact_categories")]
    pub redact_categories: RedactCategories,
```

Add to `impl Default for RouxSettings` after `group_by`:

```rust
            redact_secrets: false,
            redact_categories: RedactCategories::default(),
```

- [ ] **Step 2: Add TypeScript types**

In `src/lib/types.ts`, add before `RouxSettings`:

```typescript
export interface RedactCategories {
  apiKeys: boolean;
  credentials: boolean;
  privateKeys: boolean;
  connectionStrings: boolean;
}
```

Add to `RouxSettings` interface after `groupBy`:

```typescript
  redactSecrets: boolean;
  redactCategories: RedactCategories;
```

Add to `DEFAULT_SETTINGS` after `groupBy`:

```typescript
  redactSecrets: false,
  redactCategories: {
    apiKeys: true,
    credentials: true,
    privateKeys: true,
    connectionStrings: true,
  },
```

- [ ] **Step 3: Verify both compile**

Run: `cd src-tauri && cargo build && cd .. && npx svelte-check`
Expected: Both compile cleanly

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/settings.rs src/lib/types.ts
git commit -m "feat(redact): add redact_secrets and redact_categories settings"
```

---

### Task 3: Create the redact module with custom patterns

**Files:**
- Create: `src-tauri/src/redact.rs`
- Modify: `src-tauri/src/main.rs` (add `mod redact;`)

- [ ] **Step 1: Create `src-tauri/src/redact.rs`**

This module creates an `AnalyzerEngine` with custom `PatternRecognizer` patterns for developer secrets, and exposes a `redact_line()` function.

```rust
use redact_core::{
    AnalyzerEngine, AnonymizerConfig, AnonymizationStrategy, EntityType,
};
use std::sync::Arc;

use crate::settings::RedactCategories;

/// Entity type names for our custom patterns
const API_KEY: &str = "API_KEY";
const GITHUB_TOKEN: &str = "GITHUB_TOKEN";
const AWS_KEY: &str = "AWS_ACCESS_KEY";
const JWT_TOKEN: &str = "JWT_TOKEN";
const BEARER_TOKEN: &str = "BEARER_TOKEN";
const BASIC_AUTH: &str = "BASIC_AUTH";
const URL_CREDENTIALS: &str = "URL_CREDENTIALS";
const PRIVATE_KEY: &str = "PRIVATE_KEY";
const CONNECTION_STRING: &str = "CONNECTION_STRING";

/// Build an AnalyzerEngine with custom patterns for the enabled categories.
pub fn build_engine(categories: &RedactCategories) -> AnalyzerEngine {
    let mut engine = AnalyzerEngine::new();
    let registry = engine.recognizer_registry_mut();

    let mut recognizer = redact_core::RecognizerRegistry::default();
    // We need to build a PatternRecognizer and add it to the engine's registry.
    // Since redact-core's API uses the recognizer registry, we add patterns via
    // a new PatternRecognizer instance.

    let mut pr = redact_core::recognizers::PatternRecognizer::new();

    if categories.api_keys {
        // GitHub tokens: ghp_, gho_, ghu_, ghs_, ghr_ followed by 36 alphanumeric chars
        let _ = pr.add_pattern(
            EntityType::Custom(GITHUB_TOKEN.into()),
            r"gh[pousr]_[A-Za-z0-9]{36,}",
            0.95,
        );
        // AWS access key IDs
        let _ = pr.add_pattern(
            EntityType::Custom(AWS_KEY.into()),
            r"(?:AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{16}",
            0.95,
        );
        // Generic API keys: long hex/base64 strings near key/token/secret keywords
        let _ = pr.add_pattern_with_context(
            EntityType::Custom(API_KEY.into()),
            r"[A-Za-z0-9+/=_-]{32,}",
            0.6,
            vec!["key".into(), "token".into(), "secret".into(), "api_key".into(), "apikey".into(), "api-key".into()],
        );
        // JWT tokens
        let _ = pr.add_pattern(
            EntityType::Custom(JWT_TOKEN.into()),
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            0.9,
        );
    }

    if categories.credentials {
        // Bearer tokens in headers
        let _ = pr.add_pattern(
            EntityType::Custom(BEARER_TOKEN.into()),
            r"[Bb]earer\s+[A-Za-z0-9_.~+/=-]{20,}",
            0.9,
        );
        // Basic auth in headers (base64)
        let _ = pr.add_pattern(
            EntityType::Custom(BASIC_AUTH.into()),
            r"[Bb]asic\s+[A-Za-z0-9+/=]{20,}",
            0.85,
        );
        // Credentials in URLs: ://user:password@host
        let _ = pr.add_pattern(
            EntityType::Custom(URL_CREDENTIALS.into()),
            r"://[^:@/\s]+:[^@/\s]+@",
            0.9,
        );
    }

    if categories.private_keys {
        // PEM private key blocks
        let _ = pr.add_pattern(
            EntityType::Custom(PRIVATE_KEY.into()),
            r"-----BEGIN\s+(?:RSA\s+|EC\s+|DSA\s+|OPENSSH\s+)?PRIVATE\s+KEY-----",
            0.99,
        );
    }

    if categories.connection_strings {
        // Database connection strings with credentials
        let _ = pr.add_pattern(
            EntityType::Custom(CONNECTION_STRING.into()),
            r"(?:mongodb|postgres|postgresql|mysql|redis|amqp)://[^:@/\s]+:[^@/\s]+@[^\s]+",
            0.9,
        );
    }

    registry.add_recognizer(Arc::new(pr));
    engine
}

/// Mask config: show first 4 and last 4 chars, mask middle with '*'
fn mask_config() -> AnonymizerConfig {
    AnonymizerConfig {
        strategy: AnonymizationStrategy::Mask,
        mask_char: '*',
        mask_start_chars: 4,
        mask_end_chars: 4,
        ..Default::default()
    }
}

/// Strip ANSI escape sequences for analysis, returning the clean text
/// and a mapping of clean-text positions to original positions.
fn strip_ansi(text: &str) -> (String, Vec<usize>) {
    let mut clean = String::with_capacity(text.len());
    let mut positions = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Skip CSI sequence: ESC [ ... final_byte
            i += 2;
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // skip final byte
            }
        } else {
            positions.push(i);
            clean.push(bytes[i] as char);
            i += 1;
        }
    }
    (clean, positions)
}

/// Redact secrets in a single line of text.
/// Returns the original line with detected secrets masked.
pub fn redact_line(engine: &AnalyzerEngine, line: &str) -> String {
    // Strip ANSI for analysis
    let (clean, pos_map) = strip_ansi(line);

    let result = match engine.analyze(&clean, None) {
        Ok(r) => r,
        Err(_) => return line.to_string(),
    };

    if result.detected_entities.is_empty() {
        return line.to_string();
    }

    // Apply masks to the original text using position mapping
    let config = mask_config();
    let mut output = line.to_string();
    // Process entities in reverse order so earlier positions stay valid
    let mut entities = result.detected_entities;
    entities.sort_by(|a, b| b.start.cmp(&a.start));

    for entity in &entities {
        if entity.start >= pos_map.len() || entity.end > pos_map.len() {
            continue;
        }
        let orig_start = pos_map[entity.start];
        let orig_end = if entity.end < pos_map.len() {
            pos_map[entity.end]
        } else {
            line.len()
        };

        let secret = &line[orig_start..orig_end];
        let masked = partial_mask(secret, config.mask_char, config.mask_start_chars, config.mask_end_chars);
        output.replace_range(orig_start..orig_end, &masked);
    }

    output
}

/// Apply partial mask: show first N and last N chars, mask middle.
fn partial_mask(text: &str, mask_char: char, start_chars: usize, end_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len <= start_chars + end_chars {
        // Too short — mask everything after first 2
        let visible = 2.min(len);
        let masked_count = len.saturating_sub(visible);
        return chars[..visible].iter().collect::<String>()
            + &std::iter::repeat(mask_char).take(masked_count).collect::<String>();
    }
    let start: String = chars[..start_chars].iter().collect();
    let end: String = chars[len - end_chars..].iter().collect();
    let mid_len = len - start_chars - end_chars;
    let mid: String = std::iter::repeat(mask_char).take(mid_len).collect();
    format!("{}{}{}", start, mid, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_mask_normal() {
        assert_eq!(partial_mask("ghp_abcdefghijklmnopqr", '*', 4, 4), "ghp_**************opqr");
    }

    #[test]
    fn partial_mask_short() {
        assert_eq!(partial_mask("sk_abc", '*', 4, 4), "sk****");
    }

    #[test]
    fn redact_github_token() {
        let cats = RedactCategories { api_keys: true, credentials: false, private_keys: false, connection_strings: false };
        let engine = build_engine(&cats);
        let input = "export GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let output = redact_line(&engine, input);
        assert!(output.contains("ghp_"));
        assert!(output.contains("****"));
        assert!(!output.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"));
    }

    #[test]
    fn no_redaction_when_no_secrets() {
        let cats = RedactCategories::default();
        let engine = build_engine(&cats);
        let input = "Hello world, nothing secret here";
        assert_eq!(redact_line(&engine, input), input);
    }

    #[test]
    fn disabled_category_not_redacted() {
        let cats = RedactCategories { api_keys: false, credentials: true, private_keys: false, connection_strings: false };
        let engine = build_engine(&cats);
        let input = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        // API keys disabled, should pass through
        assert_eq!(redact_line(&engine, input), input);
    }
}
```

- [ ] **Step 2: Add `mod redact;` to main.rs**

In `src-tauri/src/main.rs`, add with the other module declarations:

```rust
mod redact;
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test redact`
Expected: All 4 tests pass

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/redact.rs src-tauri/src/main.rs
git commit -m "feat(redact): add redact module with custom secret patterns and tests"
```

---

### Task 4: Wire redaction into PTY output pipeline

**Files:**
- Modify: `src-tauri/src/pty.rs`
- Modify: `src-tauri/src/main.rs` (pass redaction config to PtyManager)

- [ ] **Step 1: Add shared redaction state to PtyManager**

In `src-tauri/src/pty.rs`, add these imports at the top:

```rust
use crate::redact;
use crate::settings::RedactCategories;
```

Add a shared redaction config struct after the existing imports:

```rust
#[derive(Clone)]
pub struct RedactConfig {
    pub enabled: Arc<AtomicBool>,
    pub categories: Arc<Mutex<RedactCategories>>,
}

impl RedactConfig {
    pub fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            categories: Arc::new(Mutex::new(RedactCategories::default())),
        }
    }
}
```

Add `redact_config: RedactConfig` field to `PtyManager`:

```rust
pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
    pending_outputs: Mutex<HashMap<String, Channel<Response>>>,
    generation: AtomicU64,
    pub redact_config: RedactConfig,
}
```

Update `PtyManager::new()`:

```rust
pub fn new() -> Self {
    Self {
        sessions: Mutex::new(HashMap::new()),
        pending_outputs: Mutex::new(HashMap::new()),
        generation: AtomicU64::new(0),
        redact_config: RedactConfig::new(),
    }
}
```

- [ ] **Step 2: Modify `spawn_reader` to accept and use redaction config**

Replace the `spawn_reader` function:

```rust
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: mpsc::Sender<PtyChunk>,
    redact_config: RedactConfig,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut line_buf = Vec::new();
        let mut engine: Option<redact_core::AnalyzerEngine> = None;
        let mut engine_categories: Option<RedactCategories> = None;

        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    // Flush remaining line buffer
                    if !line_buf.is_empty() {
                        let data = flush_line_buf(&mut line_buf, &redact_config, &mut engine, &mut engine_categories);
                        let _ = tx.send(PtyChunk::Data(data));
                    }
                    let _ = tx.send(PtyChunk::Eof);
                    break;
                }
                Ok(n) => {
                    if !redact_config.enabled.load(Ordering::Relaxed) {
                        // Redaction disabled — passthrough with no buffering
                        if tx.send(PtyChunk::Data(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    } else {
                        // Line-buffer and redact
                        line_buf.extend_from_slice(&buf[..n]);

                        // Process complete lines
                        let mut output = Vec::new();
                        while let Some(newline_pos) = line_buf.iter().position(|&b| b == b'\n') {
                            let line_bytes: Vec<u8> = line_buf.drain(..=newline_pos).collect();
                            let line = String::from_utf8_lossy(&line_bytes);
                            let redacted = redact_with_engine(&line, &redact_config, &mut engine, &mut engine_categories);
                            output.extend_from_slice(redacted.as_bytes());
                        }

                        // Flush if line_buf exceeds 8KB (no newline seen)
                        if line_buf.len() > 8192 {
                            let data = flush_line_buf(&mut line_buf, &redact_config, &mut engine, &mut engine_categories);
                            output.extend_from_slice(&data);
                        }

                        if !output.is_empty() {
                            if tx.send(PtyChunk::Data(output)).is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(_) => {
                    if !line_buf.is_empty() {
                        let data = flush_line_buf(&mut line_buf, &redact_config, &mut engine, &mut engine_categories);
                        let _ = tx.send(PtyChunk::Data(data));
                    }
                    let _ = tx.send(PtyChunk::Error);
                    break;
                }
            }
        }
    });
}

fn redact_with_engine(
    line: &str,
    config: &RedactConfig,
    engine: &mut Option<redact_core::AnalyzerEngine>,
    engine_categories: &mut Option<RedactCategories>,
) -> String {
    let current_cats = config.categories.lock().unwrap().clone();

    // Rebuild engine if categories changed
    let needs_rebuild = match engine_categories {
        Some(prev) => {
            prev.api_keys != current_cats.api_keys
            || prev.credentials != current_cats.credentials
            || prev.private_keys != current_cats.private_keys
            || prev.connection_strings != current_cats.connection_strings
        }
        None => true,
    };

    if needs_rebuild {
        *engine = Some(redact::build_engine(&current_cats));
        *engine_categories = Some(current_cats);
    }

    if let Some(eng) = engine {
        redact::redact_line(eng, line)
    } else {
        line.to_string()
    }
}

fn flush_line_buf(
    line_buf: &mut Vec<u8>,
    config: &RedactConfig,
    engine: &mut Option<redact_core::AnalyzerEngine>,
    engine_categories: &mut Option<RedactCategories>,
) -> Vec<u8> {
    let text = String::from_utf8_lossy(&line_buf);
    let redacted = redact_with_engine(&text, config, engine, engine_categories);
    line_buf.clear();
    redacted.into_bytes()
}
```

- [ ] **Step 3: Update all `spawn_reader` call sites to pass redact_config**

In `PtyManager::spawn()` (~line 315):

```rust
spawn_reader(reader, tx, self.redact_config.clone());
```

In `PtyManager::spawn_shell()` (find the `spawn_reader` call):

```rust
spawn_reader(reader, tx, self.redact_config.clone());
```

In `PtyManager::spawn_command()` (if it exists, find the `spawn_reader` call):

```rust
spawn_reader(reader, tx, self.redact_config.clone());
```

Search for all `spawn_reader(` calls in pty.rs and update them all.

- [ ] **Step 4: Sync settings to redact_config when settings are saved**

In `src-tauri/src/main.rs`, find the `cmd_update_settings` command and add after the settings are saved:

```rust
// Sync redact config to PTY manager
state.pty_manager.redact_config.enabled.store(
    settings.redact_secrets,
    std::sync::atomic::Ordering::Relaxed,
);
*state.pty_manager.redact_config.categories.lock().unwrap() = settings.redact_categories.clone();
```

Also sync on app startup after settings are loaded (in the `main` function or wherever `AppState` is constructed):

```rust
let settings = settings::load_settings();
// ... construct AppState ...
// Sync initial redact settings
app_state.pty_manager.redact_config.enabled.store(
    settings.redact_secrets,
    std::sync::atomic::Ordering::Relaxed,
);
*app_state.pty_manager.redact_config.categories.lock().unwrap() = settings.redact_categories.clone();
```

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: Compiles cleanly

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/pty.rs src-tauri/src/main.rs
git commit -m "feat(redact): wire redaction into PTY output pipeline"
```

---

### Task 5: Add Security section to Settings UI

**Files:**
- Modify: `src/lib/components/SettingsPanel.svelte`

- [ ] **Step 1: Add Security section before Debug section**

In `SettingsPanel.svelte`, add before the `<!-- Debug -->` section (~line 264):

```svelte
    <!-- Security -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Security</h3>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Redact secrets</div>
          <div class="text-[11px] text-text-muted mt-0.5">Mask API keys, tokens, and credentials in terminal output</div>
        </div>
        <button
          aria-label="Toggle secret redaction"
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.redactSecrets
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("redactSecrets", !$settings.redactSecrets)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.redactSecrets
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
      {#if $settings.redactSecrets}
        <div class="ml-2 mt-1 space-y-1.5">
          {#each [
            { key: "apiKeys", label: "API Keys & Tokens", desc: "GitHub, AWS, JWT, generic API keys" },
            { key: "credentials", label: "Credentials & Passwords", desc: "Bearer tokens, basic auth, URL passwords" },
            { key: "privateKeys", label: "Private Keys", desc: "PEM-encoded private keys" },
            { key: "connectionStrings", label: "Connection Strings", desc: "Database URLs with embedded credentials" },
          ] as cat}
            <label class="flex items-center gap-2 cursor-pointer py-0.5">
              <input
                type="checkbox"
                class="accent-accent"
                checked={$settings.redactCategories[cat.key as keyof typeof $settings.redactCategories]}
                onchange={() => {
                  const updated = { ...$settings.redactCategories };
                  updated[cat.key as keyof typeof updated] = !updated[cat.key as keyof typeof updated];
                  updateSetting("redactCategories", updated);
                }}
              />
              <div>
                <div class="text-[12px] text-text-primary">{cat.label}</div>
                <div class="text-[10px] text-text-muted">{cat.desc}</div>
              </div>
            </label>
          {/each}
        </div>
      {/if}
    </section>
```

- [ ] **Step 2: Verify it compiles**

Run: `npx svelte-check`
Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/SettingsPanel.svelte
git commit -m "feat(redact): add Security section with redaction toggles to settings UI"
```

---

### Task 6: Integration test and final verification

- [ ] **Step 1: Run all tests**

Run: `cd src-tauri && cargo test`
Expected: All tests pass including the new redact module tests

- [ ] **Step 2: Run full build**

Run: `cd src-tauri && cargo build && cd .. && npx svelte-check`
Expected: Clean compile on both sides

- [ ] **Step 3: Manual test**

1. Launch the app with `task dev`
2. Open Settings → Security section
3. Toggle "Redact secrets" ON
4. Open a terminal and run: `echo "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"`
5. Verify output shows: `ghp_****...ghij` (partially masked)
6. Toggle OFF, run the same command — verify full token visible
7. Toggle ON, disable "API Keys & Tokens" category — verify token passes through
8. Test a bearer token: `echo "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123"`
9. Verify it gets masked when "Credentials" is enabled

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(redact): secret redaction in terminal output with settings toggle"
```
