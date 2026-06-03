# Automation hooks

Roux automation hooks are shell commands that run around app-level lifecycle events: watches, worktree creation/removal, sessions, and tasks.

They are inspired by [Worktrunk hooks](https://worktrunk.dev/hook/), but they are Roux-owned. Worktrunk hooks still live in `.config/wt.toml` and run when `wt` owns a git/worktree operation. Roux hooks live in Roux config and can react to Roux-specific context such as sessions, watches, task panes, and the active worktree provider.

## Config files

Roux loads hooks from:

| Scope   | Path                             | Trust              |
| ------- | -------------------------------- | ------------------ |
| User    | `~/.config/roux/hooks.toml`      | trusted by default |
| Project | `<repo>/.config/roux/hooks.toml` | requires approval  |

Project approvals are keyed by config path, event, command name, and command text. If a project command changes, Roux treats it as a new command and requires approval again.

## Hook forms

A string is one command:

```toml
post-watch-success = "roux notify --title 'Watch passed'"
```

A table is multiple named commands in the same step:

```toml
[post-watch-failure]
notify = "roux notify --title '{{ watch.name }} failed' --level error"
log = "printf '%s\n' '{{ watch.name }} failed' >> /tmp/roux-watch.log"
```

A pipeline is an ordered sequence of `[[event]]` blocks. Blocks run in order; multiple commands inside one block run as the same step.

```toml
[[post-worktree-create]]
install = "npm ci"

[[post-worktree-create]]
dev = "npm run dev"
test = "npm test -- --watch"
```

## Event types

`pre-*` hooks are blocking. If a blocking hook exits non-zero, Roux aborts or skips the operation and returns the hook error.

`post-*` hooks run in the background. Their failures are logged but do not retroactively fail the operation that already completed.

### Watches

| Event                | When it runs                                                                               |
| -------------------- | ------------------------------------------------------------------------------------------ |
| `pre-watch-run`      | Before executing a watch check. Failure skips the check and records a failed watch result. |
| `post-watch-run`     | After every completed watch check.                                                         |
| `post-watch-change`  | Only when the watch outcome changes.                                                       |
| `post-watch-failure` | Only when the outcome transitions into failure.                                            |
| `post-watch-success` | Only when the outcome transitions into success.                                            |

### Worktrees

| Event                  | When it runs                               |
| ---------------------- | ------------------------------------------ |
| `pre-worktree-create`  | Before Roux invokes the selected provider. |
| `post-worktree-create` | After a worktree is created.               |
| `pre-worktree-remove`  | Before Roux removes a worktree.            |
| `post-worktree-remove` | After removal completes.                   |

When the effective provider is Worktrunk, Roux marks Worktrunk's provider-owned hooks as already handled in the hook context. Roux does not try to rerun Worktrunk's `pre-start`, `post-start`, `pre-remove`, or `post-remove`.

### Sessions and tasks

| Event                 | When it runs                                             |
| --------------------- | -------------------------------------------------------- |
| `post-session-create` | After Roux creates a session record and primary PTY.     |
| `pre-session-close`   | Before Roux archives a session and kills its PTYs.       |
| `post-session-close`  | After session close cleanup completes.                   |
| `pre-task-run`        | Before Roux spawns a task command.                       |
| `post-task-run`       | After Roux successfully spawns a task command.           |
| `post-task-success`   | When a task PTY exits with code `0`.                     |
| `post-task-failure`   | When a task PTY exits with any non-zero or unknown code. |

## Conditions

Add a `when` table to a hook step to run only in matching contexts:

```toml
[[post-worktree-create]]
when.provider = "worktrunk"
notify = "roux notify --title 'Worktrunk worktree ready'"

[[post-worktree-create]]
when.provider = "git"
install = "npm ci"
```

Supported conditions:

| Condition        | Values                                             |
| ---------------- | -------------------------------------------------- |
| `when.provider`  | `"git"`, `"worktrunk"`, or `"auto"` context values |
| `when.worktrunk` | `true` or `false`                                  |
| `when.scope`     | `"global"`, `"project"`, or `"session"`            |

For conditions, `when.provider = "git"` and `when.provider = "worktrunk"` match the effective provider Roux intends to use for the operation. `when.provider = "auto"` matches when the user's configured provider is `auto`. The rendered context includes both `provider` and `configured_provider` when you need to distinguish those values in a template or stdin JSON.

## Templates

Roux renders hook commands with [MiniJinja](https://docs.rs/minijinja/latest/minijinja/), a Rust implementation of Jinja-style templates:

```toml
post-watch-success = "echo '{{ watch.name }} passed in {{ cwd }}'"
post-worktree-create = "echo '{{ branch }} -> {{ worktree_path }}'"
```

Templates can use variables, conditionals, loops, expressions, and MiniJinja's built-in filters:

```toml
post-watch-failure = "{% if watch.name %}echo '{{ watch.name }} failed'{% endif %}"
post-worktree-create = "createdb {{ branch | sanitize_db }}"
```

Common variables:

| Variable              | Meaning                                            |
| --------------------- | -------------------------------------------------- |
| `hook_type`           | Event name, such as `post-watch-success`.          |
| `hook_name`           | Named command key, such as `notify`.               |
| `provider`            | Effective provider, usually `git` or `worktrunk`.  |
| `configured_provider` | User setting: `auto`, `git`, or `worktrunk`.       |
| `worktrunk`           | Boolean convenience flag.                          |
| `repo_path`           | Repository root when known.                        |
| `worktree_path`       | Worktree path when known.                          |
| `branch`              | Branch name when known.                            |
| `cwd`                 | Directory where Roux runs the hook command.        |
| `session_id`          | Roux session id when known.                        |
| `project_id`          | Roux project id when known.                        |
| `task_id`             | Task PTY id for task hooks.                        |
| `watch.id`            | Watch id for watch hooks.                          |
| `watch.name`          | Watch name for watch hooks.                        |
| `outcome`             | Current watch outcome when known.                  |
| `previous_outcome`    | Previous watch outcome when known.                 |
| `args`                | Extra tokens passed by `roux hook run ... -- ...`. |

Roux also provides Worktrunk-inspired helper filters and functions:

| Helper                    | Example                                 | Meaning                                                                    |
| ------------------------- | --------------------------------------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `sanitize`                | `{{ branch                              | sanitize }}`                                                               | Lowercase path/shell-friendly token.                                                     |
| `sanitize_hash`           | `{{ branch                              | sanitize_hash }}`                                                          | Sanitized token with a short stable hash when the original needed rewriting or was long. |
| `sanitize_db`             | `{{ branch                              | sanitize_db }}`                                                            | Lowercase database-safe identifier, capped at 63 characters.                             |
| `hash_port`               | `{{ branch                              | hash_port }}`                                                              | Stable port in the `10000..49999` range.                                                 |
| `worktree_path_of_branch` | `{{ worktree_path_of_branch("main") }}` | Looks up a local git worktree path for a branch when `repo_path` is known. |

!!! note "Shell escaping"
MiniJinja renders text; it does not automatically shell-escape values. Quote variables in commands when paths, branch names, or watch names can contain spaces or shell metacharacters.

## JSON context on stdin

Every hook command receives the full context as JSON on stdin. Use this for logic that is too complex for simple templates:

```toml
[pre-watch-run]
gate = "python3 scripts/should-run-watch.py"
```

```python
import json
import sys

ctx = json.load(sys.stdin)
watch = ctx.get("watch") or {}

if watch.get("name") == "expensive-ci" and ctx.get("provider") == "git":
    print("skip expensive CI outside worktrunk", file=sys.stderr)
    sys.exit(1)
```

For a `pre-*` hook, exiting non-zero blocks the operation. For a `post-*` hook, the failure is logged.

## Logs

Background hook output is written under:

```text
~/.config/roux/logs/hooks/
```

Each log file records:

- event name
- command name
- source (`user` or `project`)
- config path
- original and rendered command
- exit code
- stdout and stderr
- start and finish timestamps

## CLI

Show configured hooks:

```sh
roux hook show
roux hook show --repo-path ~/src/my-repo
```

Run a hook manually through the running app:

```sh
roux hook run post-watch-success --repo-path ~/src/my-repo
roux hook run post-worktree-create \
  --repo-path ~/src/my-repo \
  --worktree-path ~/src/my-repo/.worktrees/feat-x \
  --branch feat/x \
  --provider worktrunk
```

Forward extra tokens into `args`:

```sh
roux hook run post-watch-run --repo-path ~/src/my-repo -- --verbose
```

The legacy Claude status bridge still uses the same top-level group:

```sh
roux hook working
roux hook idle
roux hook attention
roux hook error
roux hook disconnected
```

## Worktrunk relationship

Use Worktrunk hooks for git/worktree lifecycle automation that should run everywhere `wt` runs.

Use Roux automation hooks for app-aware behavior:

- notifying or focusing Roux sessions
- reacting to watch outcome transitions
- starting or logging task runs
- running different automation depending on whether Roux used git or Worktrunk
- coordinating with Roux project/session IDs

The two systems are intentionally separate so Roux can be useful without `wt`, while still exposing provider context when Worktrunk is active.
