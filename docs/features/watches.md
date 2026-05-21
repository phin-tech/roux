# Watches

Watches are long-running commands whose output Roux surfaces in dedicated panes. Typical uses are test runners, build watchers, and log tails.

!!! note "Stub page"
    Detailed documentation for watches is still being written.

## What a watch is

A watch is:

- a command to run
- a working directory
- a set of lifecycle rules (restart on exit, restart on file change, manual rerun only, etc.)

Watches are managed from the command palette and settings UI.

## Watch panes

When a watch is pinned to a pane, Roux keeps the output buffer alive even if you close and reopen the pane or toggle stacking. You can rerun a watch from its pane's header.

## See also

- [Panes](panes.md)
- [CLI bridge](cli.md) — drive watches from scripts via `roux`
- [Automation hooks](hooks.md) — react to watch runs and outcome transitions
