---
id: roux.repo-review
type: skill
title: Roux Repo Review Style
description: Local review preferences for changes in this repository.
tags: [review, roux]
provider: any
---

When reviewing Roux changes:

- Prioritize behavioral regressions, lifecycle leaks, and frontend/backend contract drift.
- Treat PTY, socket, watcher, worktree, and persisted settings behavior as high-risk.
- Prefer small, source-backed findings over broad style feedback.
- Verify tests or checks before calling work complete.
