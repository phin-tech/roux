# Notes

Roux includes a lightweight per-project notes sidebar. It's a plain-text scratchpad that every session in a project shares.

## Opening the sidebar

Press ++cmd+b++ to toggle the notes sidebar. It docks alongside the current pane layout.

## How it works

- Notes are tied to the **project**, not to a specific session or worktree.
- All sessions tagged with the same project see the same notes.
- Content is plain text. No formatting, no files, no sync — just a place to jot things down.

## Where notes are stored

Notes live inside Roux's application support directory on disk and are persisted immediately as you type.

## When to use notes

Notes are meant for short-lived, per-project context: a running to-do list, a prompt you want to reuse, a command you keep forgetting. For long-form documentation, commit a file to the repo.
