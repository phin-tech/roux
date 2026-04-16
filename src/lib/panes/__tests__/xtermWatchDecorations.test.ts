import { describe, expect, it } from "vitest";

import {
  buildWatchConfigForTarget,
  findWatchTargetsInText,
} from "../xtermWatchDecorations";

describe("xtermWatchDecorations", () => {
  it("finds GitHub Action and PR watch targets in terminal text", () => {
    const targets = findWatchTargetsInText(
      [
        "Run: https://github.com/phin-tech/roux/actions/runs/12345",
        "PR: https://github.com/phin-tech/roux/pull/41",
      ].join(" "),
    );

    expect(targets).toEqual([
      {
        kind: "githubAction",
        repo: "phin-tech/roux",
        runId: 12345,
        urlEnd: 57,
      },
      {
        kind: "githubPr",
        repo: "phin-tech/roux",
        prNumber: 41,
        urlEnd: 103,
      },
    ]);
  });

  it("builds a session-scoped one-shot watch for GitHub Actions", () => {
    const target = findWatchTargetsInText(
      "https://github.com/phin-tech/roux/actions/runs/12345",
    )[0];

    expect(buildWatchConfigForTarget(target, "s1")).toEqual({
      name: "GH: phin-tech/roux #12345",
      kind: {
        type: "githubAction",
        repo: "phin-tech/roux",
        runId: 12345,
        workflow: null,
        branch: null,
      },
      mode: { type: "oneShot" },
      scope: { type: "session", sessionId: "s1" },
      notify: null,
    });
  });

  it("builds a global recurring watch for pull requests without an active session", () => {
    const target = findWatchTargetsInText(
      "https://github.com/phin-tech/roux/pull/41",
    )[0];

    expect(buildWatchConfigForTarget(target, null)).toEqual({
      name: "PR: phin-tech/roux #41",
      kind: {
        type: "githubPr",
        repo: "phin-tech/roux",
        prNumber: 41,
      },
      mode: { type: "recurring", intervalSecs: 30 },
      scope: { type: "global" },
      notify: null,
    });
  });
});
