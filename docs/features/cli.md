# CLI bridge (`roux-cli`)

Roux ships with a command-line tool, `roux-cli`, that talks to the running app over a Unix socket. It lets you script Roux from the terminal — open sessions, split panes, send text, and focus panes.

!!! note "Stub page"
    The full command reference is still being written. This page covers the basics.

## Installing

`roux-cli` is bundled inside `Roux.app`. The easiest way to put it on your `PATH` is to symlink it:

```sh
ln -sf /Applications/Roux.app/Contents/MacOS/roux-cli /usr/local/bin/roux
```

Then `roux --help` should work from any terminal.

## What it can do

- Create a new session in a given project
- Split the active pane horizontally or vertically
- Send text to a target pane (useful for pasting a prompt from a script)
- Focus a pane by id or direction
- Run a shell command in a new pane

## Example

Open a new session in the current directory:

```sh
roux session new "$PWD"
```

Split the active pane and run `npm run test:watch` inside it:

```sh
roux split --direction vertical --command "npm run test:watch"
```

See `roux --help` and `roux <subcommand> --help` for the authoritative list of commands and flags.
