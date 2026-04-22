# Panes

Panes are the basic building block of a Roux window. Every Claude Code session, shell, document, or command runner lives inside a pane.

## Splitting

- ++cmd+d++ — split horizontally (new pane to the right)
- ++cmd+shift+d++ — split vertically (new pane below)

Consecutive splits in the same direction are flattened into a single row or column, so your layout never accumulates redundant nesting.

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

## Persistence

Layouts persist across restarts. Shell panes are respawned automatically; Claude session panes are recreated empty and you can start a new session inside them.
