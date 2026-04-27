# Keyboard Shortcuts

The full set of shortcuts shipped in the `default` keymap. All shortcuts are also visible in the command palette (++cmd+k++) next to each action, so this page is for reference — you never need to memorize it.

> Roux's keyboard shortcuts are **fully customizable**. Every binding on this page is just an entry in `~/.config/roux/keymap.kdl`. You can rebind anything, layer your own keys on top of a built-in preset, or switch to the `tmux` preset entirely. See [Keymap](features/keymap.md) for the full schema.

## Panes

| Action | Shortcut |
|---|---|
| Split horizontal | ++cmd+d++ |
| Split vertical | ++cmd+shift+d++ |
| Close pane | ++cmd+w++ |
| Toggle stack | ++cmd+shift+s++ |
| Focus left | ++alt+h++ |
| Focus down | ++alt+j++ |
| Focus up | ++alt+k++ |
| Focus right | ++alt+l++ |
| Focus pane 1–9 | ++alt+1++ … ++alt+9++ |
| Focus pane 10 | ++alt+0++ |

The pane-focus shortcuts target the Nth visible pane in the active session (depth-first, left-to-right). Hold ++alt++ on its own to see each pane's digit drawn centered over it.
You can disable this overlay in **Settings → Keyboard** without disabling the shortcuts themselves.

## Leader mode

Roux also has a Vimish leader-mode surface for pane and session commands. The leader is just a regular tree in the keymap — the prefix, the children, and the HUD timing are all editable. See [Keymap → Trees, prefixes, and modes](features/keymap.md#trees-prefixes-and-modes) for how to add your own modes.

- ++cmd+;++ — open leader mode (press again to close)
- ++escape++ — exit leader mode
- ++space++ — expand into the full command palette

### Pane leader keys

- ++cmd+; w++ — pane commands
- ++h++ / ++j++ / ++k++ / ++l++ — move focus between panes
- ++s++ — split horizontally
- ++v++ — split vertically
- ++r++ — rename the active pane inline
- ++d++ — close the active pane
- ++f++ — toggle fullscreen on the active pane
- ++t++ — toggle stack when the focused pane belongs to a splittable parent

### Session leader keys

- ++cmd+; b++ — session commands
- ++n++ — new session
- ++d++ — close session
- ++r++ — reconnect session
- ++e++ — open the session worktree in your editor

### Toggle leader keys

- ++cmd+; t++ — UI toggles
- ++n++ — toggle notes
- ++s++ — toggle sessions history
- ++w++ — toggle watches
- ++i++ — toggle notifications

## Sessions and windows

| Action | Shortcut |
|---|---|
| New session | ++cmd+n++ |
| Switch to session 1–9 | ++cmd+1++ … ++cmd+9++ |
| Switch to session 10 | ++cmd+0++ |
| Settings | ++cmd+","++ |

The session-switch shortcuts target the Nth session in the sidebar's top-to-bottom order. Hold ++cmd++ on its own for a moment to see the digit for each session drawn as an overlay on top of the card.
You can disable this overlay in **Settings → Keyboard** while keeping ++cmd+digit++ session switching enabled.

## Editor

| Action | Shortcut |
|---|---|
| Open multi-line prompt editor | ++cmd+shift+e++ |
| Open with clipboard | ++cmd+shift+v++ |
| Insert into terminal | ++cmd+enter++ |
| Cancel editor | ++escape++ |

The multi-line editor is available in shell and command panes. ++cmd+enter++ inserts the edited text without auto-executing, so you can review it in the terminal before pressing Enter. See [Editor](features/editor.md) for details on the transform toolbar and how the editor positions itself.

## Navigation

| Action | Shortcut |
|---|---|
| Command palette | ++cmd+k++ |
| Toggle notes | ++cmd+b++ |
| Toggle sessions history | ++cmd+; t s++ |
| Toggle notifications | ++cmd+i++ |
| Toggle watches | ++cmd+shift+w++ |
| Toggle sidebar | ++cmd+"\\"++ |
| Reload keymap | *(from palette)* |

The native menu bar uses the same active keymap, so menu accelerators follow your current preset and any custom overrides after you reload the keymap.

## Customizing shortcuts

Every shortcut on this page comes from `~/.config/roux/keymap.kdl`, written for you on first launch. You can:

- **Switch to a different preset.** Replace the file's contents with `preset "tmux"` (or `preset "default"` to revert) and run **Reload Keymap** from the palette.
- **Override a single binding.** Add a `bind "..." "command.id"` line below the preset reference; it overrides the preset's binding for that key.
- **Drop a binding.** `unbind "Cmd+KeyB"` removes that direct bind from the preset.
- **Add a new mode.** Declare a `tree "..." { ... }` and a `prefix "..." tree="..."` to create your own chord group. Add `sticky` for Zellij-style modes that stay armed; add `passthrough` to let unbound keys reach the focused terminal.

Reload via ++cmd+k++ → **Reload Keymap** to pick up changes without a restart. See the full [Keymap reference](features/keymap.md) for grammar, modifier syntax, command IDs, and worked examples.
