import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import {
  activeSidebar,
  armPaneHints,
  armSessionHints,
  boardFullscreen,
  closeBoardFullscreen,
  editingWorkItemId,
  newWorkItemEditor,
  openNewWorkItemEditor,
  openWorkItemEditor,
  closeWorkItemEditor,
  closePinned,
  closeSidebar,
  hidePaneHints,
  hideSessionHints,
  isPinned,
  openBoardFullscreen,
  openSidebar,
  pinnedSidebar,
  pinSidebar,
  PINNABLE_SIDEBARS,
  showPaneHints,
  showSessionHints,
  toggleBoardFullscreen,
  toggleSidebar,
  unpinSidebar,
} from "../ui";

describe("showSessionHints", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    hideSessionHints();
  });

  afterEach(() => {
    hideSessionHints();
    vi.useRealTimers();
  });

  it("stays false until the hold delay elapses", () => {
    armSessionHints(200);
    expect(get(showSessionHints)).toBe(false);
    vi.advanceTimersByTime(199);
    expect(get(showSessionHints)).toBe(false);
    vi.advanceTimersByTime(1);
    expect(get(showSessionHints)).toBe(true);
  });

  it("hide cancels a pending arm", () => {
    armSessionHints(200);
    vi.advanceTimersByTime(100);
    hideSessionHints();
    vi.advanceTimersByTime(200);
    expect(get(showSessionHints)).toBe(false);
  });

  it("hide turns off an already-visible overlay", () => {
    armSessionHints(50);
    vi.advanceTimersByTime(50);
    expect(get(showSessionHints)).toBe(true);
    hideSessionHints();
    expect(get(showSessionHints)).toBe(false);
  });

  it("arming again while a timer is pending is a no-op", () => {
    armSessionHints(200);
    vi.advanceTimersByTime(150);
    armSessionHints(200); // should not reset the timer
    vi.advanceTimersByTime(50);
    expect(get(showSessionHints)).toBe(true);
  });
});

describe("sidebar pin-slot state", () => {
  beforeEach(() => {
    closeSidebar();
    unpinSidebar();
  });

  afterEach(() => {
    closeSidebar();
    unpinSidebar();
  });

  describe("openSidebar / closeSidebar / toggleSidebar", () => {
    it("openSidebar sets active and leaves pinned null", () => {
      openSidebar("notes");
      expect(get(activeSidebar)).toBe("notes");
      expect(get(pinnedSidebar)).toBeNull();
    });

    it("closeSidebar clears active but preserves pinned", () => {
      pinSidebar("notes");
      openSidebar("watches");
      closeSidebar();
      expect(get(activeSidebar)).toBeNull();
      expect(get(pinnedSidebar)).toBe("notes");
    });

    it("toggleSidebar clears active when id matches active", () => {
      openSidebar("watches");
      toggleSidebar("watches");
      expect(get(activeSidebar)).toBeNull();
    });

    it("toggleSidebar unpins when id matches pinned", () => {
      pinSidebar("notes");
      toggleSidebar("notes");
      expect(get(pinnedSidebar)).toBeNull();
    });

    it("toggleSidebar activates a panel that is neither active nor pinned", () => {
      toggleSidebar("watches");
      expect(get(activeSidebar)).toBe("watches");
    });

    it("opening a pinned panel while active slot has another keeps both visible", () => {
      pinSidebar("notes");
      openSidebar("watches");
      expect(get(pinnedSidebar)).toBe("notes");
      expect(get(activeSidebar)).toBe("watches");
    });
  });

  describe("pinSidebar / unpinSidebar / isPinned", () => {
    it("pinSidebar sets the pinned slot for a pinnable panel", () => {
      pinSidebar("notes");
      expect(get(pinnedSidebar)).toBe("notes");
      expect(isPinned("notes")).toBe(true);
    });

    it("pinSidebar is a no-op for non-pinnable panels (settings)", () => {
      pinSidebar("settings");
      expect(get(pinnedSidebar)).toBeNull();
    });

    it("pinSidebar is a no-op for non-pinnable panels (docs)", () => {
      pinSidebar("docs");
      expect(get(pinnedSidebar)).toBeNull();
    });

    it("unpinSidebar clears the pinned slot", () => {
      pinSidebar("notes");
      unpinSidebar();
      expect(get(pinnedSidebar)).toBeNull();
    });

    it("unpinSidebar promotes the former pin to active (anchor wins over transient)", () => {
      pinSidebar("notes");
      openSidebar("watches");
      unpinSidebar();
      expect(get(pinnedSidebar)).toBeNull();
      expect(get(activeSidebar)).toBe("notes");
    });

    it("unpinSidebar leaves active unchanged when nothing was pinned", () => {
      openSidebar("watches");
      unpinSidebar();
      expect(get(activeSidebar)).toBe("watches");
    });

    it("unpinSidebar preserves a docs takeover (doesn't close docs)", () => {
      pinSidebar("notes");
      openSidebar("docs");
      unpinSidebar();
      expect(get(pinnedSidebar)).toBeNull();
      expect(get(activeSidebar)).toBe("docs");
    });

    it("unpinSidebar preserves a settings takeover (doesn't close settings)", () => {
      pinSidebar("notes");
      openSidebar("settings");
      unpinSidebar();
      expect(get(pinnedSidebar)).toBeNull();
      expect(get(activeSidebar)).toBe("settings");
    });

    it("closePinned clears only the pinned slot, leaving active untouched", () => {
      pinSidebar("notes");
      openSidebar("watches");
      closePinned();
      expect(get(pinnedSidebar)).toBeNull();
      expect(get(activeSidebar)).toBe("watches");
    });

    it("closePinned on a solo pinned panel clears the pin (no promotion)", () => {
      pinSidebar("notes");
      closePinned();
      expect(get(pinnedSidebar)).toBeNull();
      expect(get(activeSidebar)).toBeNull();
    });

    it("pinning the currently-active panel clears active so the panel sits in the pin slot only", () => {
      openSidebar("notes");
      pinSidebar("notes");
      expect(get(pinnedSidebar)).toBe("notes");
      expect(get(activeSidebar)).toBeNull();
    });

    it("PINNABLE_SIDEBARS includes the lightweight panels", () => {
      expect(PINNABLE_SIDEBARS.has("notes")).toBe(true);
      expect(PINNABLE_SIDEBARS.has("watches")).toBe(true);
      expect(PINNABLE_SIDEBARS.has("tasks")).toBe(true);
      expect(PINNABLE_SIDEBARS.has("notifications")).toBe(true);
    });

    it("PINNABLE_SIDEBARS excludes heavy panels", () => {
      expect(PINNABLE_SIDEBARS.has("settings")).toBe(false);
      expect(PINNABLE_SIDEBARS.has("docs")).toBe(false);
    });
  });

  describe("takeover behavior for Settings / Docs", () => {
    it("opening settings while another panel is pinned leaves pinned state intact", () => {
      pinSidebar("notes");
      openSidebar("settings");
      expect(get(pinnedSidebar)).toBe("notes");
      expect(get(activeSidebar)).toBe("settings");
    });

    it("opening docs while another panel is pinned leaves pinned state intact", () => {
      pinSidebar("watches");
      openSidebar("docs");
      expect(get(pinnedSidebar)).toBe("watches");
      expect(get(activeSidebar)).toBe("docs");
    });
  });

  describe("collapse-to-icons: opening a panel unhides the dock", () => {
    it("openSidebar unhides the dock when collapsed", async () => {
      const { sidebarLayout, hideSidebar, showSidebar } = await import(
        "../sidebarLayout"
      );
      showSidebar();
      hideSidebar();
      expect(get(sidebarLayout).hidden).toBe(true);
      openSidebar("watches");
      expect(get(sidebarLayout).hidden).toBe(false);
    });

    it("toggleSidebar activating a panel unhides the dock", async () => {
      const { sidebarLayout, hideSidebar, showSidebar } = await import(
        "../sidebarLayout"
      );
      showSidebar();
      hideSidebar();
      expect(get(sidebarLayout).hidden).toBe(true);
      toggleSidebar("watches");
      expect(get(sidebarLayout).hidden).toBe(false);
      expect(get(activeSidebar)).toBe("watches");
    });

    it("toggleSidebar dismissing the active panel does not touch hidden", async () => {
      const { sidebarLayout, showSidebar } = await import("../sidebarLayout");
      showSidebar();
      openSidebar("watches");
      toggleSidebar("watches");
      expect(get(activeSidebar)).toBeNull();
      expect(get(sidebarLayout).hidden).toBe(false);
    });

    it("pinSidebar unhides the dock when collapsed", async () => {
      const { sidebarLayout, hideSidebar, showSidebar } = await import(
        "../sidebarLayout"
      );
      showSidebar();
      hideSidebar();
      pinSidebar("notes");
      expect(get(sidebarLayout).hidden).toBe(false);
      expect(get(pinnedSidebar)).toBe("notes");
    });
  });
});

describe("showPaneHints", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    hidePaneHints();
  });

  afterEach(() => {
    hidePaneHints();
    vi.useRealTimers();
  });

  it("has independent timer state from the session hint store", () => {
    armSessionHints(200);
    armPaneHints(200);
    vi.advanceTimersByTime(200);
    expect(get(showSessionHints)).toBe(true);
    expect(get(showPaneHints)).toBe(true);
    hideSessionHints();
    expect(get(showSessionHints)).toBe(false);
    expect(get(showPaneHints)).toBe(true);
  });
});

describe("boardFullscreen", () => {
  afterEach(() => {
    closeBoardFullscreen();
  });

  it("starts closed", () => {
    expect(get(boardFullscreen)).toBe(false);
  });

  it("opens, closes, and toggles", () => {
    openBoardFullscreen();
    expect(get(boardFullscreen)).toBe(true);
    openBoardFullscreen();
    expect(get(boardFullscreen)).toBe(true);

    closeBoardFullscreen();
    expect(get(boardFullscreen)).toBe(false);

    toggleBoardFullscreen();
    expect(get(boardFullscreen)).toBe(true);
    toggleBoardFullscreen();
    expect(get(boardFullscreen)).toBe(false);
  });

  it("is independent of the sidebar slots", () => {
    pinSidebar("sessions");
    openBoardFullscreen();
    expect(get(boardFullscreen)).toBe(true);
    expect(get(pinnedSidebar)).toBe("sessions");

    closeBoardFullscreen();
    expect(get(pinnedSidebar)).toBe("sessions");
    unpinSidebar();
  });
});

describe("editingWorkItemId", () => {
  afterEach(() => {
    closeWorkItemEditor();
  });

  it("starts null and tracks the open/close target", () => {
    expect(get(editingWorkItemId)).toBeNull();
    openWorkItemEditor("wi-1");
    expect(get(editingWorkItemId)).toBe("wi-1");
    openWorkItemEditor("wi-2");
    expect(get(editingWorkItemId)).toBe("wi-2");
    closeWorkItemEditor();
    expect(get(editingWorkItemId)).toBeNull();
  });

  it("separates create mode from editing mode", () => {
    openNewWorkItemEditor({ status: "review" });
    expect(get(editingWorkItemId)).toBeNull();
    expect(get(newWorkItemEditor)).toEqual({ status: "review" });

    openWorkItemEditor("wi-1");
    expect(get(newWorkItemEditor)).toBeNull();
    expect(get(editingWorkItemId)).toBe("wi-1");

    closeWorkItemEditor();
    expect(get(editingWorkItemId)).toBeNull();
    expect(get(newWorkItemEditor)).toBeNull();
  });
});
