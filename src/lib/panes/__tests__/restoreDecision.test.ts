import { describe, expect, it } from "vitest";

import type { PaneDescriptor } from "../persistence";
import { decidePaneRestore } from "../restoreDecision";

function descriptor(
  overrides: Partial<PaneDescriptor> = {},
): PaneDescriptor {
  return {
    id: "pane-1",
    type: "shell",
    ptyId: "pty-1",
    ...overrides,
  };
}

describe("decidePaneRestore", () => {
  it("attaches a primary pane only when the session PTY is live", () => {
    expect(
      decidePaneRestore({
        descriptor: descriptor({ id: "s1-main", ptyId: "s1" }),
        sessionId: "s1",
        livePtyIds: new Set(["s1"]),
      }),
    ).toEqual({
      kind: "attach",
      ptyId: "s1",
      panePtyId: "s1",
      terminalState: { kind: "attached", ptyId: "s1" },
    });
  });

  it("keeps a stale primary pane disconnected instead of treating the old PTY as attachable", () => {
    expect(
      decidePaneRestore({
        descriptor: descriptor({ id: "s1-main", ptyId: "s1" }),
        sessionId: "s1",
        livePtyIds: new Set(),
      }),
    ).toEqual({
      kind: "empty",
      panePtyId: "s1",
      terminalState: { kind: "empty" },
    });
  });

  it("empties stale non-primary shell panes instead of respawning them during cold restore", () => {
    expect(
      decidePaneRestore({
        descriptor: descriptor({ id: "shell-pane", ptyId: "old-pty" }),
        sessionId: "s1",
        livePtyIds: new Set(["s1"]),
      }),
    ).toEqual({
      kind: "empty",
      panePtyId: "",
      terminalState: { kind: "empty" },
    });
  });

  it("does not attach but preserves PTY identity when live PTY inventory is unknown", () => {
    expect(
      decidePaneRestore({
        descriptor: descriptor({ id: "shell-pane", ptyId: "maybe-live" }),
        sessionId: "s1",
        livePtyIds: null,
      }),
    ).toEqual({
      kind: "empty",
      panePtyId: "maybe-live",
      terminalState: { kind: "empty" },
    });
  });

  it("does not strip command panes when live PTY inventory is unknown", () => {
    expect(
      decidePaneRestore({
        descriptor: descriptor({
          id: "cmd-pane",
          type: "command",
          ptyId: "maybe-live-command",
          command: "npm test",
        }),
        sessionId: "s1",
        livePtyIds: null,
      }),
    ).toEqual({
      kind: "empty",
      panePtyId: "maybe-live-command",
      terminalState: { kind: "empty" },
    });
  });

  it("strips command panes unless their PTY is known live", () => {
    expect(
      decidePaneRestore({
        descriptor: descriptor({
          id: "cmd-pane",
          type: "command",
          ptyId: "old-command",
          command: "npm test",
        }),
        sessionId: "s1",
        livePtyIds: new Set(["s1"]),
      }),
    ).toEqual({ kind: "strip" });
  });
});
