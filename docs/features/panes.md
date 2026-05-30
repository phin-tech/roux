# Panes

Panes are the basic building block of a Roux window. Every Claude Code session, shell, document, or command runner lives inside a pane.

## Splitting

- ++cmd+d++ — split horizontally (new pane to the right)
- ++cmd+shift+d++ — split vertically (new pane below)

Consecutive splits in the same direction are flattened into a single row or column, so your layout never accumulates redundant nesting.

## Split with profile

Roux can also split the current pane and seed the new shell with a specific spawn profile.

Available command-palette actions include:

- **Split Right with Profile…**
- **Split Down with Profile…**
- **Split Right → Claude**
- **Split Right → Codex**

The profile-driven path is what lets a new pane start as more than a plain shell. A profile can attach setup commands, startup commands, and environment variables before you start using the pane.

This is the fastest way to drop a second agent or a specialized shell next to the current pane without opening the New Session dialog.

## Stacking

Toggle stacking with ++cmd+shift+s++. Stacked panes behave like Zellij tabs: the active pane fills the space while inactive panes collapse into a title bar strip. Click a title bar to activate that pane.

## Focus

Move focus between panes using Alt + vim direction keys:

- ++alt+h++ left
- ++alt+j++ down
- ++alt+k++ up
- ++alt+l++ right

For structural pane commands, use leader mode with ++cmd+;++. The pane group under ++w++ gives you a compact, keyboard-first command surface for focus movement, splits, rename, close, fullscreen, and stack toggling. Context-sensitive actions only appear when they make sense for the currently focused pane.

Examples:

- ++cmd+; w s++ split horizontally
- ++cmd+; w v++ split vertically
- ++cmd+; w r++ rename the active pane inline
- ++cmd+; w d++ close the active pane

## Closing

++cmd+w++ closes the focused pane.

- If the pane hosts a Claude session, the session is stopped.
- If the pane hosts a shell or command PTY, Roux **detaches** that terminal by default instead of killing it. The process keeps running in the background and can be re-attached later.

This matters because closing a pane and killing the underlying terminal are not the same operation in Roux.

## Attaching and detaching terminals

Shell and command panes can own a PTY independently of the pane that is currently displaying it.

### What detach means

When a PTY is **detached**:

- the process keeps running
- its output continues to accumulate in the background
- it is no longer bound to a visible pane
- you can attach it to another shell or command pane later

Detaching is the default behavior when you close a pane that has an attached PTY.

### How to re-attach

Roux currently exposes terminal re-attachment in three places:

- **Empty shell/command pane UI** — when a pane has no terminal attached, it shows **Attach Terminal...**
- **Command palette** — run **Attach Terminal...**
- **Native menu bar** — **Pane** → **Attach Terminal...**

The picker shows terminals from the **current session** only. It can include:

- terminals already attached to another pane
- terminals currently detached and running in the background

Choosing one moves that PTY into the focused pane. If the terminal was already attached somewhere else, Roux clears the old pane’s binding and reattaches the PTY to the new pane.

### Detached terminal badges

Session cards show a small detached-terminal count when a session has background PTYs that are no longer attached to a pane. If unread output arrived while detached, the badge is highlighted.

### Kill vs close

Use **Kill Terminal** when you want to stop the underlying PTY immediately.

- **Close pane** removes the pane and, by default, detaches the PTY
- **Kill Terminal** stops the PTY process itself

This distinction is especially useful for long-running shells or command panes that you want to keep alive while reorganizing the layout.

## Command panes

Roux can run an ad-hoc shell command in its own dedicated pane.

Open one from the command palette with **Run Command**.

Command panes:

- start as a split next to the active pane
- stream terminal output like any other terminal-backed pane
- show elapsed time while running
- show success/error status when the process exits
- support rerun from the pane header after completion

This is useful for one-off test runs, build commands, or long-running scripts you want visible in the layout without manually opening a shell first.

## Other pane types

Not every pane is an interactive shell.

Roux also supports:

- **Markdown document panes** via **Open Document**
- **Notes panes** via **Open Notes Pane (Horizontal/Vertical)**
- **Command panes** via **Run Command**

All of these participate in the same split/stack/focus system as shell and agent panes.

## Persistence

Layouts persist across restarts. Shell panes are respawned automatically; Claude session panes are recreated empty and you can start a new session inside them.
