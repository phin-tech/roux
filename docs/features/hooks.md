# Hooks

Hooks let you attach local automation to Roux events.

Roux emits normalized events for things it already knows about, and user-defined local scripts can react to those events by calling `roux-cli` to open sessions, create panes, send input, or push notifications.

## What hooks are for

Hooks unify a few kinds of automation under one model:

- provider-driven status updates such as Claude or future agent hooks
- Roux-native lifecycle events such as session or pane creation
- watch-driven events such as a PR watch changing outcome
- local workflow automation that should stay on the user's machine

The value of hooks is not an embedded scripting language. The value is:

- normalized events
- supervised hook execution with timeouts
- trust gating for repo-local hooks
- `roux-cli` primitives that are safe enough for automation
- useful logs when a hook runs, skips, times out, or fails

## Config locations

Roux supports two hook scopes:

- global hooks in `~/.config/roux/hooks.kdl`
- repo hooks in `<repo>/.roux/hooks.kdl`

Repo hooks only run for trusted workspaces.

## How hooks work

The model is:

- hook registration lives in `hooks.kdl`
- Roux normalizes events into one shared event envelope
- Roux spawns the hook as a supervised child process
- Roux writes the full event JSON to the hook process on `stdin`
- the hook script uses `roux-cli` to perform any action inside Roux

This deliberately keeps the control boundary outside the app. Hooks should automate Roux through the same public CLI or socket surface that any other local script would use.

## Example use cases

### PR watch opens a fixer session

When a PR watch reports new review comments or failing CI, a hook can:

1. inspect the event payload
2. decide whether the event is actionable
3. use `roux-cli` to ensure a background session exists for that PR
4. open a shell or command pane to start repair work

### Attention hook

When an agent enters an attention state, a hook can:

- push a richer local notification
- log the event to another local tool
- open a notes pane or helper session in the same workspace

### Session bootstrap

When a session is created, a hook can:

- create a companion shell pane
- start a repo-specific watcher
- annotate the session through future CLI primitives

## Config shape

The format is KDL, consistent with Roux layouts and keymap configuration.

```kdl
hooks {
  hook "pr-watch-auto-fix" {
    on "watch.completed"

    run {
      command "python3"
      arg ".roux/scripts/pr_watch.py"
      cwd "worktree"
      timeout-ms 300000
    }

    policy {
      concurrency "replace"
    }
  }
}
```

The config stays intentionally small:

- `on` decides which event kinds can spawn the hook
- `run` describes how to launch the process
- `policy` controls timeout and concurrency behavior

Fine-grained applicability lives in the hook script itself. A hook may receive an event and immediately decide "not applicable" and exit successfully.

## Filters

Hooks do not assume a rich matcher language.

Built-in pre-spawn filters stay very small and cheap. The first candidates are:

- `equals`
- `in`
- `exists`

Anything more ambitious than that should wait for real examples.

## Event envelope

The hook process receives a normalized event object with fields like:

- event id
- event kind
- timestamp
- origin or causation metadata
- session and pane identifiers where available
- repo root and worktree path where available
- event-specific payload data

The event envelope exists so hook scripts can be boring and deterministic. They should not need to scrape UI state or infer context from filenames.

## Safety model

Hooks run as supervised child processes, not in-process plugins.

The safety rules are:

- every run has a timeout
- Roux owns process cleanup
- Roux captures stdout and stderr for logs
- repo-local hooks only run in trusted workspaces
- hook-caused events are tracked so Roux can prevent accidental loops

That last point matters. A hook that opens a session in response to an event should not recursively trigger itself forever.

## What hooks are not

Hooks are not:

- an in-app plugin API
- an embedded language runtime such as Scheme or Rhai
- a replacement for writing a normal local script
- a promise that Roux will execute arbitrary repo code without trust gating

If matching logic ever grows more complex, that should happen only after the simple model proves insufficient in practice.

## Related features

- [CLI bridge](cli.md)
- [Watches](watches.md)
- [Sessions](sessions.md)
- [Worktrees](worktrees.md)
