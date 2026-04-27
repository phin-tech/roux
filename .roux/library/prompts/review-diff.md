---
id: roux.review-diff
type: prompt
title: Review Current Diff
description: Ask an agent to review the active repo diff for correctness and missing tests.
tags: [review, git]
variables:
  - name: focus
    label: Review focus
    default: correctness, regressions, and missing tests
---

Review the current git diff in this repo.

Focus on {{ focus }}.

Start with concrete findings. Include file paths and line numbers when possible.
