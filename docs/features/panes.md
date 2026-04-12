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

++cmd+w++ closes the focused pane. If a pane hosts a Claude session, the session is stopped. If it hosts a shell, the shell is terminated.

## Persistence

Layouts persist across restarts. Shell panes are respawned automatically; Claude session panes are recreated empty and you can start a new session inside them.
