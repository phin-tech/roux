# Multi-Line Prompt Editor

The multi-line editor is a floating panel for preparing and cleaning up shell commands before inserting them into a terminal pane. It's especially useful for:

- pasting multi-line CLI commands from documentation or LLM output
- removing markdown code fence markers (``` ```)
- collapsing wrapped continuation lines (backslash newlines)
- stripping prompt prefixes ($, ❯, #, >) from copy-pasted snippets
- fixing smart quotes that snuck in from rich-text sources
- joining wrapped lines for compact one-liners

## Opening the editor

### From scratch
- ++cmd+shift+e++ opens an empty editor seeded from the shell's current prompt (if available)

### From clipboard
- ++cmd+shift+v++ opens the editor pre-populated with your clipboard contents

The editor is only available in **shell panes** and **command panes** that have an attached terminal.

## Submitting and canceling

- ++cmd+enter++ inserts the edited text into the terminal **without auto-executing**. You'll see the command in the terminal, ready to review before you press Enter.
- ++escape++ closes the editor without writing anything.

## Transform toolbar

The editor includes six one-shot text transforms. Click a button to apply; you can undo and apply multiple times within the same editing session.

| Transform | What it does |
|---|---|
| Join lines | Collapse newlines and surrounding whitespace into a single line |
| Unwrap \ | Remove trailing backslash-newline continuations (\\n becomes space) |
| Strip $ / ❯ | Remove leading prompt markers: `$`, `❯`, `#`, `>`, plus one trailing space |
| Strip ``` | Remove leading and trailing markdown code fence markers |
| Smart → straight | Replace curly/smart quotes (`"`, `'`) with straight ASCII quotes |
| Trim | Strip leading and trailing whitespace |

## Editor position and dragging

The editor appears as a floating panel that doesn't block the rest of your layout.

- **Smart positioning**: For new shells (minimal output), the editor appears near the top. For active shells (scrolled content), it appears near the bottom so it doesn't hide what you're doing.
- **Dragging**: Click and drag the header bar to move the panel. Position persists across app restarts.
- **Viewport safety**: The panel is always kept partially visible so you can grab it back if it drifts off-screen during a resize.

## Text editing features

The editor uses [CodeMirror](https://codemirror.net/) for syntax highlighting and editing:

- **Syntax highlighting**: Shell syntax is highlighted for readability
- **Undo/redo**: ++cmd+z++ and ++cmd+shift+z++ (or ++ctrl+y++ for redo) work as expected
- **Standard keybindings**: Cut, copy, paste, select-all, and other common text commands work normally

## Keyboard reference

| Shortcut | Action |
|---|---|
| ++cmd+shift+e++ | Open empty editor |
| ++cmd+shift+v++ | Open with clipboard |
| ++cmd+enter++ | Insert into terminal |
| ++escape++ | Cancel |
| ++cmd+z++ | Undo |
| ++cmd+shift+z++ | Redo |

All these keybindings are customizable via `~/.config/roux/keymap.kdl`. See [Keymap](keymap.md) for how to rebind them.
