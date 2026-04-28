import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri", () => ({
  writeToSession: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../paneAtPoint", () => ({
  paneAtPoint: vi.fn(),
}));

import { writeToSession } from "$lib/tauri";
import { paneAtPoint } from "../paneAtPoint";
import { handleFileDrop } from "../handleFileDrop";
import { paneInstances, resetInstances, type PaneInstance } from "$lib/panes/instances";
import { focusedPaneId, resetFocus } from "$lib/panes/focus";

function makePane(id: string, ptyId: string | null): PaneInstance {
  return {
    id,
    type: "shell",
    ptyId: ptyId ?? "",
    terminalState: ptyId
      ? { kind: "attached", ptyId }
      : { kind: "empty" },
    unlisteners: [],
  };
}

function setPanes(panes: PaneInstance[]) {
  const map = new Map<string, PaneInstance>();
  for (const p of panes) map.set(p.id, p);
  paneInstances.set(map);
}

describe("handleFileDrop", () => {
  beforeEach(() => {
    resetInstances();
    resetFocus();
    vi.mocked(writeToSession).mockClear();
    vi.mocked(paneAtPoint).mockReset();
  });

  it("writes the formatted path to the pane found at the cursor", async () => {
    setPanes([makePane("pane-1", "pty-1")]);
    vi.mocked(paneAtPoint).mockReturnValue("pane-1");

    await handleFileDrop({
      paths: ["/tmp/file.txt"],
      position: { x: 10, y: 20 },
    });

    expect(writeToSession).toHaveBeenCalledTimes(1);
    expect(writeToSession).toHaveBeenCalledWith("pty-1", "/tmp/file.txt");
  });

  it("shell-quotes paths with spaces and joins multiple", async () => {
    setPanes([makePane("pane-1", "pty-1")]);
    vi.mocked(paneAtPoint).mockReturnValue("pane-1");

    await handleFileDrop({
      paths: ["/a/b", "/c d"],
      position: { x: 0, y: 0 },
    });

    expect(writeToSession).toHaveBeenCalledWith("pty-1", "/a/b '/c d'");
  });

  it("does not append a newline (so the user can keep typing)", async () => {
    setPanes([makePane("pane-1", "pty-1")]);
    vi.mocked(paneAtPoint).mockReturnValue("pane-1");

    await handleFileDrop({
      paths: ["/tmp/file"],
      position: { x: 0, y: 0 },
    });

    const data = vi.mocked(writeToSession).mock.calls[0][1];
    expect(data.endsWith("\r")).toBe(false);
    expect(data.endsWith("\n")).toBe(false);
  });

  it("falls back to the focused pane when no pane is under the cursor", async () => {
    setPanes([makePane("pane-1", "pty-1"), makePane("pane-2", "pty-2")]);
    focusedPaneId.set("pane-2");
    vi.mocked(paneAtPoint).mockReturnValue(null);

    await handleFileDrop({
      paths: ["/tmp/x"],
      position: { x: 0, y: 0 },
    });

    expect(writeToSession).toHaveBeenCalledWith("pty-2", "/tmp/x");
  });

  it("falls back to the focused pane when the pane under cursor has no attached PTY", async () => {
    setPanes([makePane("pane-empty", null), makePane("pane-focus", "pty-focus")]);
    focusedPaneId.set("pane-focus");
    vi.mocked(paneAtPoint).mockReturnValue("pane-empty");

    await handleFileDrop({
      paths: ["/tmp/x"],
      position: { x: 0, y: 0 },
    });

    expect(writeToSession).toHaveBeenCalledWith("pty-focus", "/tmp/x");
  });

  it("is a no-op when no pane is under cursor and no pane is focused", async () => {
    setPanes([makePane("pane-1", "pty-1")]);
    vi.mocked(paneAtPoint).mockReturnValue(null);
    focusedPaneId.set(null);

    await handleFileDrop({
      paths: ["/tmp/x"],
      position: { x: 0, y: 0 },
    });

    expect(writeToSession).not.toHaveBeenCalled();
  });

  it("is a no-op for empty paths array", async () => {
    setPanes([makePane("pane-1", "pty-1")]);
    vi.mocked(paneAtPoint).mockReturnValue("pane-1");

    await handleFileDrop({
      paths: [],
      position: { x: 0, y: 0 },
    });

    expect(writeToSession).not.toHaveBeenCalled();
  });

  it("is a no-op when the resolved pane id has no instance in the store", async () => {
    setPanes([]);
    vi.mocked(paneAtPoint).mockReturnValue("ghost-pane");

    await handleFileDrop({
      paths: ["/tmp/x"],
      position: { x: 0, y: 0 },
    });

    expect(writeToSession).not.toHaveBeenCalled();
  });
});
