import { describe, expect, it } from "vitest";

import {
  buildWatchConfigForTarget,
  findWatchTargetsInText,
  projectOffset,
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

  it("anchors urlEnd past PR path suffixes (/files, /commits, /checks)", () => {
    const url = "https://github.com/phin-tech/roux/pull/41/files";
    const targets = findWatchTargetsInText(url);
    expect(targets).toHaveLength(1);
    expect(targets[0].kind).toBe("githubPr");
    expect(targets[0].urlEnd).toBe(url.length);
  });

  it("anchors urlEnd past query strings and fragments", () => {
    const url1 = "https://github.com/phin-tech/roux/pull/41?foo=bar";
    expect(findWatchTargetsInText(url1)[0].urlEnd).toBe(url1.length);

    const url2 = "https://github.com/phin-tech/roux/pull/41#issuecomment-123";
    expect(findWatchTargetsInText(url2)[0].urlEnd).toBe(url2.length);
  });

  it("anchors urlEnd past Action job and attempt suffixes", () => {
    const job = "https://github.com/phin-tech/roux/actions/runs/12345/job/9876";
    expect(findWatchTargetsInText(job)[0].urlEnd).toBe(job.length);

    const attempts =
      "https://github.com/phin-tech/roux/actions/runs/12345/attempts/2";
    expect(findWatchTargetsInText(attempts)[0].urlEnd).toBe(attempts.length);
  });

  it("matches both http and https, with optional www subdomain", () => {
    const httpUrl = "http://www.github.com/phin-tech/roux/pull/41";
    const targets = findWatchTargetsInText(httpUrl);
    expect(targets).toHaveLength(1);
    expect(targets[0]).toMatchObject({
      kind: "githubPr",
      repo: "phin-tech/roux",
      prNumber: 41,
    });
  });

  it("stops matching at whitespace so adjacent text doesn't bleed in", () => {
    const text =
      "https://github.com/phin-tech/roux/pull/41/files extra words after";
    const targets = findWatchTargetsInText(text);
    expect(targets).toHaveLength(1);
    expect(targets[0].urlEnd).toBe(
      "https://github.com/phin-tech/roux/pull/41/files".length,
    );
  });

  it("projects logical-string offsets back to (visual line, column) for wrapped URLs", () => {
    // Simulate three visual segments of width 40, where a URL straddles
    // the second and third segment. The decoration should anchor to the
    // trailing segment.
    const segments = [
      { absLine: 100, yOffset: -2, length: 40 },
      { absLine: 101, yOffset: -1, length: 40 },
      { absLine: 102, yOffset: 0, length: 40 },
    ];
    // Offset 95 falls on segment 2 (40+40=80, 80+40=120). Column = 95-80 = 15.
    expect(projectOffset(segments, 95)).toEqual({
      absLine: 102,
      yOffset: 0,
      column: 15,
    });
    // Offset right at a segment boundary — 80 — lands on segment 2 col 0
    // (we project the *end* of the URL, which is exclusive).
    expect(projectOffset(segments, 80)).toEqual({
      absLine: 101,
      yOffset: -1,
      column: 40,
    });
  });

  it("clamps offset projection past the logical end to the trailing segment", () => {
    const segments = [{ absLine: 5, yOffset: 0, length: 30 }];
    expect(projectOffset(segments, 999)).toEqual({
      absLine: 5,
      yOffset: 0,
      column: 30,
    });
  });
});
