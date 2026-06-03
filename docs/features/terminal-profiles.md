# Terminal Profiles

Terminal profiles describe how Roux should prepare a new shell before any
interactive setup or startup command is typed into the PTY. Roux owns session
intent: profile selection, stable environment, and preflight checks. Project
tools such as `direnv`, `mise`, `asdf`, `nvm`, and shell rc files still own
project-local environment setup after the shell starts.

## Terminal Defaults

Preferences → Terminal includes defaults applied to every newly spawned PTY:

- Environment rules.
- A `Before shell starts` command.
- Plain split behavior for **Split Horizontal** and **Split Vertical**.

Settings apply only to new PTYs. Existing shells are not mutated.

Plain split behavior controls what the plain split commands do:

- `plainShell` — always spawn a plain shell.
- `appDefaultProfile` — spawn with the configured default agent profile.
- `activePaneProfile` — reuse the focused pane's profile when available.
- `askEveryTime` — open the existing split-with-profile picker.

The explicit **Split Horizontal with Profile...** and **Split Vertical with
Profile...** commands always show the picker, regardless of this setting.

## Environment Rules

Profile and terminal-default `env` values are JSON objects keyed by variable
name. The editor validates the shape before saving.

Legacy string values still work:

```json
{
  "AWS_PROFILE": "prod"
}
```

Structured rules support four modes:

```json
{
  "AWS_PROFILE": { "mode": "value", "value": "prod" },
  "PATH": { "mode": "inherit" },
  "AWS_REGION": { "mode": "unset" },
  "TOKEN": { "mode": "command", "command": "op read op://Work/token/value" }
}
```

Rule behavior:

- `value` sets the variable to the exact configured string.
- `inherit` leaves the currently resolved value alone if one exists.
- `unset` removes the variable from the spawned process environment.
- `command` runs a non-interactive command before spawn and sets the variable
  from trimmed stdout.

Command-derived values are not logged by default. If a command rule fails,
shell creation aborts and the error names the variable without printing stdout
or stderr.

## Preflight Commands

`beforeShellStarts` runs before the PTY is spawned, after environment rules are
resolved. Use it for readiness checks or authentication steps that need to
complete before the shell exists.

Example AWS SSO profile preflight:

```sh
aws sts get-caller-identity --profile prod >/dev/null 2>&1 || aws sso login --profile prod
```

Terminal-default preflight runs before profile preflight. Both run with the
final resolved environment.

## Precedence

Environment is resolved in this order:

```text
daemon base env -> global terminal env -> Roux session env -> profile env -> per-launch overrides
```

`inherit` means inherit from the resolved environment at that point. It does
not inspect live exports inside an already-running shell.

## Example AWS SSO Profile

```json
{
  "id": "aws-prod",
  "name": "AWS prod",
  "source": "user",
  "env": {
    "AWS_PROFILE": { "mode": "value", "value": "prod" },
    "AWS_REGION": { "mode": "value", "value": "us-east-1" }
  },
  "beforeShellStarts": "aws sts get-caller-identity --profile prod >/dev/null 2>&1 || aws sso login --profile prod",
  "startupCommand": "claude",
  "startupBehavior": "autoRun"
}
```

Roux resolves the env and runs the preflight before spawning the PTY. After the
PTY exists, Roux may type the true shell setup/startup commands. It no longer
types `export KEY=value` into the frontend terminal for profile env.
