# Multiline Editor

The multiline editor is a compact prompt editor docked to the bottom of the active terminal pane. It is for preparing terminal input before sending it to a shell or agent TUI, without fighting the terminal's single-line prompt.

Use it when you want to:

- paste or revise multi-line commands from docs, chat, or terminal output
- edit selected terminal text before running it again
- send a block of text to Claude Code without losing the editor
- review command corrections before applying them
- keep pane context visible while editing

The editor is intentionally a plain textarea, not CodeMirror. That keeps focus, selection, clipboard, and terminal submission behavior predictable inside xterm panes.

## Open And Close

| Action | Default shortcut |
|---|---|
| Toggle editor for the focused terminal pane | ++ctrl+g++ |
| Toggle editor from anywhere in the app | ++cmd+shift+e++ |
| Open editor with clipboard contents | ++cmd+shift+v++ |
| Close editor without sending | ++escape++ or ++ctrl+g++ while the editor has focus |

The editor is available for shell and command panes with an attached terminal. It is scoped to one pane at a time and docks inside that pane, above the status bar.

Clicking the terminal area while the editor is open returns focus to the editor, unless you are selecting/copying terminal text.

## Seed Text

Roux can open the editor with useful starting text:

- **Selected terminal text**: select text in a terminal pane, then press ++ctrl+g++. The editor opens with the selection loaded.
- **Clipboard text**: press ++cmd+shift+v++ to open with clipboard contents. Clipboard text takes priority over terminal selection.
- **Empty prompt**: press ++ctrl+g++ with no selection to start from a blank editor.

## Send Behavior

| Action | Default shortcut |
|---|---|
| Send editor text to the terminal and keep the editor open | ++cmd+enter++ |
| Insert a newline inside the editor | ++shift+enter++, ++ctrl+enter++, or ++alt+enter++ |

When you send:

- the text is written to the focused pane's attached PTY
- Roux also sends Enter, so the shell or agent sees it as if you pressed Enter in the terminal
- the editor stays open for follow-up edits
- shell panes use xterm input for single-line commands so normal shells receive the text reliably
- multi-line shell input and agent input use paste-style insertion before Enter

## Context Chips

The compact header shows small chips for the context Roux knows about the target pane:

- input target: `shell` or `claude`
- cwd: the pane working-directory basename, falling back to the session worktree
- git branch
- worktrunk state when available: `dirty`, ahead/behind counts, and `locked`
- spawn profile name when the pane was created from a profile

These chips are informational. Clickable actions stay limited to the close button, command correction pill, keyboard-shortcut hint, and Send button.

## Command Corrections

In shell mode, Roux checks the first command line for a small set of common mistakes and shows a subtle **Fix** pill when it has a suggestion. Corrections are click-to-apply; Roux does not rewrite your input silently.

Current first-pass corrections include:

| Input | Suggested correction |
|---|---|
| `gti status` | `git status` |
| `git statsu` | `git status` |
| `git comit` | `git commit` |
| `git chekout` | `git checkout` |
| `npm dev` | `npm run dev` |
| `npm build` | `npm run build` |
| `npm check` | `npm run check` |
| `npm lint` | `npm run lint` |

Valid npm lifecycle shortcuts such as `npm test` and `npm start` are left alone.

## Editor Keybindings

These shortcuts work while focus is inside the multiline editor:

| Shortcut | Action |
|---|---|
| ++cmd+enter++ | Send text to the target terminal and keep editor open |
| ++shift+enter++ | Insert newline |
| ++ctrl+enter++ | Insert newline |
| ++alt+enter++ | Insert newline |
| ++ctrl+c++ | Clear the editor when there is no selected text |
| ++ctrl+u++ | Copy and clear the current line |
| ++cmd+shift+k++ | Clear selected lines, or the current line when no text is selected |
| ++alt+backspace++ | Delete word left |
| ++ctrl+w++ | Delete word left |
| ++ctrl+k++ | Delete to line end |
| ++cmd+backspace++ | Delete to line start |
| ++cmd+delete++ | Delete to line end |
| ++escape++ | Close without sending |
| ++ctrl+g++ | Close without sending |

Normal textarea behavior still applies for ordinary typing, paste, selection, undo/redo, copy, and select-all.

## Terminal Selection And Copy

Terminal selection still works while the editor is open:

- dragging in the terminal can select terminal output
- copying uses the selected terminal text
- clicking the terminal without a selection focuses the editor again
- pressing ++ctrl+g++ with selected terminal text reopens/seeds the editor from that selection

## Customizing Global Shortcuts

The global shortcuts that open the editor come from `~/.config/roux/keymap.kdl`:

```kdl
bind "Ctrl+KeyG"       "pane.open-multiline-editor"
bind "Cmd+Shift+KeyE"  "pane.open-multiline-editor"
bind "Cmd+Shift+KeyV"  "pane.open-multiline-editor-with-clipboard"
```

Reload via ++cmd+k++ -> **Reload Keymap** after editing the file. See [Keymap](keymap.md) for the full keymap schema.
