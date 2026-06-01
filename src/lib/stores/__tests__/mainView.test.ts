import { describe, expect, it, afterEach } from "vitest";
import { get } from "svelte/store";
import {
  closeMainView,
  mainViewActive,
  mainViewRoute,
  openMainView,
  toggleMainView,
} from "../mainView";

describe("mainViewRoute", () => {
  afterEach(() => {
    closeMainView();
  });

  it("starts closed", () => {
    expect(get(mainViewRoute)).toBeNull();
    expect(get(mainViewActive)).toBe(false);
  });

  it("opens, replaces, and closes routes", () => {
    openMainView({ kind: "board" });
    expect(get(mainViewRoute)).toEqual({ kind: "board" });
    expect(get(mainViewActive)).toBe(true);

    openMainView({ kind: "sessionDetail", sessionId: "session-1" });
    expect(get(mainViewRoute)).toEqual({
      kind: "sessionDetail",
      sessionId: "session-1",
    });

    openMainView({ kind: "externalTool", runId: "lazygit:s1" });
    expect(get(mainViewRoute)).toEqual({
      kind: "externalTool",
      runId: "lazygit:s1",
    });

    closeMainView();
    expect(get(mainViewRoute)).toBeNull();
    expect(get(mainViewActive)).toBe(false);
  });

  it("toggles only the matching route", () => {
    toggleMainView({ kind: "board" });
    expect(get(mainViewRoute)).toEqual({ kind: "board" });

    toggleMainView({ kind: "sessionDetail", sessionId: "session-1" });
    expect(get(mainViewRoute)).toEqual({
      kind: "sessionDetail",
      sessionId: "session-1",
    });

    toggleMainView({ kind: "sessionDetail", sessionId: "session-2" });
    expect(get(mainViewRoute)).toEqual({
      kind: "sessionDetail",
      sessionId: "session-2",
    });

    toggleMainView({ kind: "externalTool", runId: "tool:a" });
    expect(get(mainViewRoute)).toEqual({
      kind: "externalTool",
      runId: "tool:a",
    });

    toggleMainView({ kind: "externalTool", runId: "tool:b" });
    expect(get(mainViewRoute)).toEqual({
      kind: "externalTool",
      runId: "tool:b",
    });

    toggleMainView({ kind: "externalTool", runId: "tool:b" });
    expect(get(mainViewRoute)).toBeNull();
  });
});
