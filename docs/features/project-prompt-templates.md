# Project Prompt Templates

Project prompts support [Minijinja](https://github.com/mitsuhiko/minijinja)-style variables. Roux stores the prompt as a raw template, then renders it each time a supported agent profile starts.

Rendered prompts are appended automatically for:

| Provider | Startup injection        |
| -------- | ------------------------ |
| Claude   | `--append-system-prompt` |
| Codex    | `-c instructions=...`    |

Other profiles are not modified automatically. They can still read `ROUX_PROJECT_PROMPT`, which contains the saved project prompt text.

## Preview

The New/Edit Project dialog has a manual **Preview** button under the project prompt field. Pick a session blueprint, then preview to render the template with that blueprint's repo, branch, worktree, and profile values.

Preview uses these fallback values before a real session exists:

| Value                   | Preview fallback                                         |
| ----------------------- | -------------------------------------------------------- |
| `session.id`            | `preview`                                                |
| `session.worktree_path` | blueprint `worktreePath`, otherwise blueprint `repoRoot` |
| `session.worktree_name` | last path segment of `session.worktree_path`             |
| `session.branch`        | blueprint `branch`, otherwise `null`                     |
| `other_sessions`        | current live sessions in the same project                |

## Variables

| Variable                | Description                                                      |
| ----------------------- | ---------------------------------------------------------------- |
| `project.id`            | Project id, or `null` in a new unsaved project preview           |
| `project.name`          | Project name                                                     |
| `project.repo_roots`    | Repository roots configured on the project                       |
| `project.context_paths` | Project context paths                                            |
| `session.id`            | Roux session id                                                  |
| `session.name`          | Session name                                                     |
| `session.repo_root`     | Repository root for the session                                  |
| `session.worktree_path` | Directory where the agent starts                                 |
| `session.worktree_name` | Last path segment of `session.worktree_path`                     |
| `session.branch`        | Branch or worktree name when known                               |
| `session.is_worktree`   | `true` when the session uses a worktree                          |
| `session.blueprint_id`  | Project blueprint id when spawned from a blueprint               |
| `profile.id`            | Spawn profile id                                                 |
| `profile.name`          | Spawn profile display name                                       |
| `profile.provider`      | `claude`, `codex`, or `null`                                     |
| `model.name`            | Roux's configured default model, or `null`                       |
| `model.family`          | Same provider family as `profile.provider`                       |
| `paths.sessions_folder` | Current session worktree directory                               |
| `other_sessions`        | Live sessions in the same project, excluding the current session |

Each item in `other_sessions` has the same fields as `session`.

## Examples

Full context sample:

```jinja
Project: {{ project.name }}
Session: {{ session.name }}
Branch: {{ session.branch or "none" }}
Worktree: {{ session.worktree_name }}
Folder: {{ paths.sessions_folder }}
Model: {{ model.name or "default" }} / {{ model.family or "unknown" }}

Other sessions:
{% for s in other_sessions %}
- {{ s.name }} on {{ s.branch or "none" }} in {{ s.worktree_name }}
{% else %}
- none
{% endfor %}
```

Branch-aware instructions:

```jinja
You are working on {{ session.branch or "the current branch" }}.
Workspace: {{ session.worktree_path }}
Model: {{ model.name or "default" }} ({{ model.family or "unknown family" }})
```

Coordinate with sibling sessions:

```jinja
Other live sessions in this project:
{% for s in other_sessions %}
- {{ s.name }} on {{ s.branch or "unknown branch" }} at {{ s.worktree_path }}
{% else %}
- No other sessions are running.
{% endfor %}
```

Use project context paths:

```jinja
Project context:
{% for path in project.context_paths %}
- {{ path }}
{% else %}
- No project context paths configured.
{% endfor %}
```

## Errors

Malformed templates and missing or misspelled variables block profile startup instead of sending raw template text to the agent. Use **Preview** before saving when adding loops, conditionals, or new variables.

Changing a project prompt does not rewrite already-running agent commands. Spawn a new session, reconnect, or re-run the profile to use the updated template.
