# Roux vs RMUX

A system/design comparison, focused on what Roux could adopt from RMUX.

> **Sources.** RMUX: [rmux.io](https://rmux.io/), the [`helvesec/rmux`](https://github.com/helvesec/rmux) README (v0.2.0, May 2026), and the [Show HN thread](https://news.ycombinator.com/item?id=48219918). Roux: the `roux` SKILL.md and `src-tauri/src/{socket.rs, cli.rs, mcp.rs, pane_state.rs, mailbox/, subscriptions/, providers/}`.

## Context

RMUX launched as "the multiplexer engine for the agentic era." It targets the same problem Roux's CLI/socket bridge and multi-agent layer already address — letting *code*, not just a keyboard, drive terminal sessions. This doc maps where the two systems differ at a design level and, primarily, which of RMUX's ideas are worth adopting in Roux. It deliberately skips implementation trivia (RMUX uses `ratatui`, etc.) in favor of system design.

## TL;DR

- **Both** are Rust-cored systems for driving terminals programmatically in the agent era, exposing a local socket + CLI.
- **RMUX = headless engine.** A Tokio daemon with three surfaces over one protocol: a tmux-compatible CLI (~90 commands), a typed async Rust SDK, and a `ratatui` embedding widget. Its differentiators are *read/await* primitives — stable pane IDs, structured `snapshot()`, `wait_for_text()` locator-style waits ("Playwright for terminals") — plus line-event streaming, headless/SSH operation, and cross-platform parity (Windows ConPTY + named pipes, no WSL). It drives *any* CLI/TUI and is positioned as a library ("you can rewrite Zellij with rmux").
- **Roux = opinionated agent workbench.** A native Tauri + Svelte desktop app: pane tree (splits + stacked tabs), git-worktree-backed sessions, xterm.js, and a far richer *coordination* layer than RMUX — agent aliases, a durable addressed mailbox, a pub/sub bus, an Obsidian-compatible notes vault, an MCP server, multi-provider support (Claude/Codex), notifications, watches/tasks/docs.
- **The overlap** is the "agents drive terminals" surface. **The divergence** is philosophy: RMUX optimizes programmatic control of arbitrary terminals; Roux optimizes humans + Claude-Code agents collaborating in one window.
- **What's worth adopting,** in priority order: (1) snapshot + `wait_for_text` read/await primitives, (2) line-level output streaming, (3) a typed SDK over the socket, (4) optional tmux-command aliases, (5) headless/daemon operation (strategic, not a quick win).

## What Roux could adopt from RMUX

Ordered by value-to-effort. Each item: what RMUX does → why it matters for Roux → what Roux has today → proposed shape → effort/risk.

### 1. Structured pane snapshot + `wait_for_text` / locator-style waits — **highest value**

- **RMUX:** stable pane IDs; `snapshot()` returns typed pane state (content + dimensions + cursor); `wait_for_text()` blocks until expected output appears, with a timeout. This is the core "Playwright for terminals" idea — no more `grep` + `sleep` to know when a command finished.
- **Why it matters:** this is the single biggest capability gap. When a Roux agent orchestrates a sibling session it does so *blind* — and the skill itself warns "sending input mid-turn can interrupt the other agent." There is no await/assert primitive, so cross-session driving is fire-and-hope.
- **Roux today:** `session send` types blind; `session poll` dumps session *metadata*, not the live terminal buffer. No `capture-pane`/`wait` verb exists (verified: absent from `cli.rs`/`socket.rs`). **But** the backend already maintains live pane snapshots internally (`pane_state.rs::save_live_pane_state`), so the hard plumbing partly exists.
- **Proposed shape:**
  - `roux-cli pane snapshot [--pane ID]` → JSON of the visible buffer (+ cursor, dims), defaulting to `$ROUX_PANE_ID`.
  - `roux-cli pane wait [--pane ID] --for "<text|regex>" [--timeout 30s]` → blocks until match or timeout; exit code signals which.
  - Turns blind `send` + coarse `poll` into reliable **send → wait → read**.
- **Effort:** medium. **Risk:** low — additive CLI verbs over the existing socket and existing live-snapshot machinery.

### 2. Line-level output streaming / subscription

- **RMUX:** "streamed output & line events," distinct from tmux's plain-text capture — agents observe state changes as they happen.
- **Why it matters:** complements #1. An orchestrator should be able to *react* to a sibling's output as it arrives instead of polling on a timer.
- **Roux today:** poll-only for siblings.
- **Proposed shape:** `roux-cli pane tail [--pane ID] --follow`, or a socket subscription channel. Roux already has a `subscriptions/` + bus module to model the stream/backpressure on.
- **Effort:** medium. **Risk:** low–medium (PTY buffering, backpressure).

### 3. Typed SDK over the socket (not just a CLI)

- **RMUX:** `rmux-sdk` crate — typed async handles: `ensure_session(...)`, `pane.send_text().await`, `pane.wait_for_text().await`. Type-safe orchestration scripts and TUI acceptance tests.
- **Why it matters:** Roux agents currently shell out to `$ROUX_CLI` and hand-parse JSON. Fine for one-offs; awkward for real orchestration scripts or tests.
- **Roux today:** CLI + JSON only.
- **Proposed shape:** a thin typed client wrapping the socket protocol. A **TypeScript** client is the most natural fit (Node/Svelte ecosystem, and the agent tooling around Claude); a Rust client is optional. Lower priority than #1/#2 — the CLI already covers most needs, so this is types + ergonomics, not new capability.
- **Effort:** medium–high. **Risk:** ongoing maintenance — the SDK must track the socket protocol.

### 4. tmux-command compatibility / familiar vocabulary — *optional*

- **RMUX:** implements ~90 tmux commands, so existing muscle memory, keybindings, and scripts port directly.
- **Why it matters:** Roux's verbs are bespoke (`session create`, `split`, `shell`). Anyone with tmux automation can't reuse it.
- **Proposed shape:** a small alias layer mapping a handful of tmux verbs (`new-session`, `split-window`, `send-keys`, `capture-pane`) onto Roux's CLI — *not* full 90-command parity. Roux isn't trying to replace tmux; the goal is a low barrier for porting scripts.
- **Effort:** low–medium for a subset. **Risk:** low; scope-creep if chasing full parity.

### 5. Headless / daemon / SSH operation — **strategic, biggest lift**

- **RMUX:** a pure Tokio daemon with no GUI; the engine *is* the product, and it runs over SSH.
- **Why it matters:** Roux ties *all* automation to a running desktop GUI ("If the CLI reports 'Roux is not running', nothing works"). That blocks headless/remote/CI use of the coordination layer.
- **Proposed shape:** evaluate whether the Rust core (sessions, PTYs, socket, mailbox, bus) can run as a standalone daemon that the Tauri GUI *attaches to*, rather than hosting. This is an architecture decision, not a feature — flag it as a direction to debate, not a quick win.
- **Effort:** high. **Risk:** high (re-architecting the backend/GUI boundary). **Needs an explicit decision.**

### 6. Cross-platform parity & "drive any TUI" framing — *mostly validation/positioning*

- RMUX leans on ConPTY + named pipes (no WSL) and pitches "drive / acceptance-test any CLI or TUI from code." Roux already uses `portable-pty` and a Windows named endpoint, so the plumbing largely exists — the gap is testing + positioning, not missing tech. Low priority; treat as a validation task.

## Where Roux is already ahead

Don't lose this in the comparison — RMUX is younger and narrower.

- **Native GUI workbench:** pane tree (splits + stacked tabs), visual focus, xterm.js. RMUX has nothing here for humans (its only UI story is the embeddable `ratatui` widget for *building* TUIs).
- **Git worktrees as a first-class session concept** (`--worktree-branch`, `--from REF`).
- **A coordination fabric RMUX lacks entirely:** agent **aliases** (stable addressable identities bound to panes), a durable **mailbox** (addressed, threaded, ack'd mail with kinds: task/result/question/fyi/signal), a pub/sub **bus** (topic broadcasts), and an Obsidian-compatible **notes vault** (global/project/repo/session scopes, tag search). RMUX's "multi-agent orchestration" is *demo code*; Roux's is a built-in durable substrate.
- **MCP server**, **multi-provider** (Claude/Codex), notifications, watches, tasks, docs.
- Roux is a *product with opinions*; RMUX is an *engine/library* you build on.

## The core philosophical difference

RMUX is a headless engine — "when the keyboard is the user, tmux still fits; when code is the user, RMUX takes over" — you assemble orchestration on top of its read/await/stream primitives. Roux is an opinionated agent workbench: a human-facing GUI plus an agent-facing coordination layer, purpose-built for humans and Claude-Code agents sharing one window. **The adoptable ideas are almost entirely RMUX's read/await/stream primitives — the part that makes driving a terminal from code *precise* — grafted onto Roux's richer coordination layer and GUI.** Roux doesn't need RMUX's engine; it needs RMUX's *inspection surface*.

## Recommended next steps

1. Spike #1 (`pane snapshot` + `pane wait`) behind the existing socket — it's the highest-leverage gap and reuses `save_live_pane_state`.
2. Decide #5 (headless daemon) as a strategic question before it's forced by a feature request.
3. Treat #2/#3/#4 as follow-ons gated on #1's design.
