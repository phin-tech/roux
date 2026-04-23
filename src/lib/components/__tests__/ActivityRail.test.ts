import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { fireEvent, render, screen } from "@testing-library/svelte";
import {
  activeSidebar,
  closeSidebar,
  pinnedSidebar,
  pinSidebar,
  unpinSidebar,
} from "$lib/stores/ui";
import ActivityRail from "../ActivityRail.svelte";

describe("ActivityRail", () => {
  beforeEach(() => {
    closeSidebar();
    unpinSidebar();
  });

  afterEach(() => {
    closeSidebar();
    unpinSidebar();
  });

  it("renders an icon button for each sidebar item", () => {
    render(ActivityRail);
    for (const label of [
      /notes/i,
      /watches/i,
      /tasks/i,
      /sessions/i,
      /docs/i,
      /notifications/i,
      /settings/i,
    ]) {
      expect(screen.getByRole("button", { name: label })).toBeDefined();
    }
  });

  it("clicking the Notes icon activates the notes sidebar", async () => {
    render(ActivityRail);
    await fireEvent.click(screen.getByRole("button", { name: /notes/i }));
    expect(get(activeSidebar)).toBe("notes");
  });

  it("clicking the active icon closes the sidebar", async () => {
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /watches/i });
    await fireEvent.click(btn);
    expect(get(activeSidebar)).toBe("watches");
    await fireEvent.click(btn);
    expect(get(activeSidebar)).toBeNull();
  });

  it("shift-clicking a pinned icon unpins it", async () => {
    pinSidebar("notes");
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /notes/i });
    await fireEvent.click(btn, { shiftKey: true });
    expect(get(pinnedSidebar)).toBeNull();
  });

  it("clicking a pinned icon without shift is a no-op", async () => {
    pinSidebar("notes");
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /notes/i });
    await fireEvent.click(btn);
    expect(get(pinnedSidebar)).toBe("notes");
    expect(get(activeSidebar)).toBeNull();
  });

  it("right-clicking a pinnable icon pins it", async () => {
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /watches/i });
    await fireEvent.contextMenu(btn);
    expect(get(pinnedSidebar)).toBe("watches");
  });

  it("right-clicking a pinned icon unpins it", async () => {
    pinSidebar("watches");
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /watches/i });
    await fireEvent.contextMenu(btn);
    expect(get(pinnedSidebar)).toBeNull();
  });

  it("right-clicking Settings does not pin it (not pinnable)", async () => {
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /settings/i });
    await fireEvent.contextMenu(btn);
    expect(get(pinnedSidebar)).toBeNull();
  });

  it("right-clicking Docs does not pin it (not pinnable)", async () => {
    render(ActivityRail);
    const btn = screen.getByRole("button", { name: /docs/i });
    await fireEvent.contextMenu(btn);
    expect(get(pinnedSidebar)).toBeNull();
  });

  it("clicking Tasks while Notes is pinned leaves Notes pinned and makes Tasks active", async () => {
    pinSidebar("notes");
    render(ActivityRail);
    await fireEvent.click(screen.getByRole("button", { name: /tasks/i }));
    expect(get(pinnedSidebar)).toBe("notes");
    expect(get(activeSidebar)).toBe("tasks");
  });

  it("opening Docs while Notes is pinned preserves the pin (takeover)", async () => {
    pinSidebar("notes");
    render(ActivityRail);
    await fireEvent.click(screen.getByRole("button", { name: /docs/i }));
    expect(get(pinnedSidebar)).toBe("notes");
    expect(get(activeSidebar)).toBe("docs");
  });
});
