# Secret Redaction in Terminal Output

## Context

Terminal sessions in Roux can display sensitive data — API keys, tokens, credentials, private keys — that users may not want visible on screen (e.g. screen sharing, recordings, shoulder surfing). This feature adds automatic detection and partial masking of secrets in PTY output, controlled by a settings toggle.

## Design

### Settings

Two new settings fields:

```typescript
redactSecrets: boolean              // master toggle, default false
redactCategories: {
  apiKeys: boolean,                 // GitHub tokens, AWS keys, generic API keys, JWTs
  credentials: boolean,             // passwords in URLs, basic auth headers, bearer tokens
  privateKeys: boolean,             // PEM blocks (RSA, EC, etc.)
  connectionStrings: boolean,       // DB connection strings with embedded credentials
}
```

Default: all categories `true` when `redactSecrets` is enabled.

### Interception Point

Redaction happens in `spawn_reader()` in `src-tauri/src/pty.rs`. This is the thread that reads raw bytes from the PTY master fd and sends them to the flusher. It's the single point before any buffering or frontend delivery.

### Line Buffering

PTY output arrives in arbitrary 4KB chunks that can split a token mid-stream. When redaction is enabled, the reader accumulates bytes into a line buffer and only forwards complete lines (on `\n`). Incomplete trailing data is held until the next read. This adds negligible latency (terminal output arrives fast) but ensures secrets aren't missed due to chunk boundaries.

When redaction is disabled, bytes pass through unchanged with no buffering overhead.

### Detection Engine

Use `redact-core` crate (Apache 2.0, from censgate/redact). It provides an `AnalyzerEngine` with regex-based pattern detection for 36+ entity types. We configure it to only detect the categories the user has enabled.

Entity type mapping:
- **apiKeys** — `AWS_ACCESS_KEY`, `GITHUB_TOKEN`, `API_KEY`, `JWT`
- **credentials** — `PASSWORD`, `BASIC_AUTH`, `BEARER_TOKEN`, `URL_CREDENTIALS`
- **privateKeys** — `PRIVATE_KEY`, `PEM_CERTIFICATE`
- **connectionStrings** — `CONNECTION_STRING`, `DATABASE_URL`

(Exact entity type names depend on what redact-core exposes; we'll map at integration time.)

### Masking Strategy

Partial mask preserving first 4 and last 4 characters:

```
ghp_abc123...xyz789abcdef  →  ghp_****...cdef
AKIAIOSFODNN7EXAMPLE       →  AKIA****MPLE
Bearer eyJhbGciOiJIUzI...  →  Bear****...XYZ1
```

For secrets shorter than 12 characters: show first 2, mask the rest.

```
sk_test_abc  →  sk****
```

### Architecture

```
PTY process
    ↓ raw bytes
spawn_reader() thread
    ↓ if redactSecrets enabled:
    │   accumulate into line buffer
    │   on complete line → AnalyzerEngine.analyze()
    │   for each detected entity → partial mask in-place
    │   send masked line as PtyChunk::Data
    ↓ if disabled:
    │   send raw bytes unchanged
spawn_flusher()
    ↓ batched bytes
Tauri channel → frontend → xterm.write()
```

### Accessing Settings from PTY Thread

The PTY manager already has access to `AppState`. We'll pass the redaction setting as a shared `Arc<AtomicBool>` (for the master toggle) and an `Arc<Mutex<RedactCategories>>` (for category config) to each reader thread at spawn time. When the user toggles settings in the frontend, `updateSettings` updates these atomics so active PTY readers pick up the change without restart.

### Settings UI

Add a "Security" section in the settings panel with:
- "Redact Secrets" toggle switch
- When enabled, show 4 category checkboxes indented below:
  - API Keys & Tokens
  - Credentials & Passwords
  - Private Keys
  - Connection Strings

### Files to Modify

1. **`src-tauri/Cargo.toml`** — add `redact-core` dependency
2. **`src-tauri/src/pty.rs`** — line buffer + redaction in `spawn_reader()`, shared settings
3. **`src-tauri/src/settings.rs`** — `RedactCategories` struct, add fields to `RouxSettings`
4. **`src/lib/types.ts`** — TypeScript `RedactCategories` interface, update `RouxSettings`
5. **`src/lib/stores/settings.ts`** — defaults for new fields
6. **Settings UI component** — add Security section with toggles

### Performance

- `redact-core` uses compiled regex patterns. Per-line analysis on typical terminal lines (< 200 chars) should be sub-millisecond.
- Line buffering adds at most one read-cycle of latency (~microseconds).
- When disabled: zero overhead (raw passthrough, no buffering).

### Edge Cases

- **Binary output** (e.g. `cat` on a binary file): lossy UTF-8 decode handles this gracefully. Redaction patterns won't match on binary garbage, so it passes through.
- **ANSI escape sequences**: Secrets may be interspersed with color codes. We strip ANSI escapes before analysis, then apply masks to the original text at the correct offsets.
- **Very long lines** (no newline): Flush the buffer if it exceeds 8KB to prevent unbounded memory growth, even if no newline seen.

## Verification

1. `cargo build` — confirms Rust compiles with redact-core
2. `npm run check` — confirms TypeScript types
3. Manual test: enable redaction, echo a GitHub token in terminal, verify partial mask appears
4. Toggle off, echo same token, verify it shows in full
5. Toggle individual categories on/off, verify only matching types are masked
6. Run `cat` on a large file to verify no performance degradation
