import { beforeEach, describe, expect, it } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import { get } from "svelte/store";
import { tick } from "svelte";
import SplitPaneHarness from "./SplitPaneHarness.svelte";
import { createPane, resetInstances } from "$lib/panes/instances";
import {
  collectLeafIds,
  getLayout,
  initSessionLayout,
  insertLeaf,
  resetLayouts,
  sessionLayouts,
} from "$lib/panes/layout";
import { resetFocus } from "$lib/panes/focus";
import { draggedPaneId, dropTarget, resetPaneDrag } from "$lib/stores/paneDrag";

class ResizeObserverStub {
  observe() {}
  disconnect() {}
}

function createDataTransfer(): DataTransfer {
  return {
    dropEffect: "move",
    effectAllowed: "all",
    files: [] as unknown as FileList,
    items: [] as unknown as DataTransferItemList,
    types: [],
    clearData() {},
    getData() {
      return "";
    },
    setData() {},
    setDragImage() {},
  } as DataTransfer;
}

describe("SplitPane drag and drop", () => {
  beforeEach(() => {
    globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
    resetLayouts();
    resetInstances();
    resetFocus();
    resetPaneDrag();

    createPane({ id: "p1", type: "shell", ptyId: "pty-1", name: "one" });
    createPane({ id: "p2", type: "shell", ptyId: "pty-2", name: "two" });

    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
  });

  it("shows an overlay and reorders panes on drop", async () => {
    const { container } = render(SplitPaneHarness, { sessionId: "s1" });
    const sourceHandle = container.querySelector('[data-pane-id="p2"] [data-drag-handle="true"]');
    const targetPane = container.querySelector('[data-drop-pane-id="p1"]');

    expect(sourceHandle).toBeTruthy();
    expect(targetPane).toBeTruthy();

    Object.defineProperty(targetPane!, "getBoundingClientRect", {
      configurable: true,
      value: () => ({
        left: 0,
        top: 0,
        width: 120,
        height: 80,
        right: 120,
        bottom: 80,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }),
    });

    const dataTransfer = createDataTransfer();
    await fireEvent.dragStart(sourceHandle!, { dataTransfer });
    expect(get(draggedPaneId)).toBe("p2");
    await fireEvent.dragOver(targetPane!, { clientX: 2, clientY: 40, dataTransfer });
    expect(get(dropTarget)).toEqual({ paneId: "p1", side: "left" });
    await tick();

    const overlay = container.querySelector('[data-drop-side="left"]');
    expect(overlay).toBeTruthy();

    await fireEvent.drop(targetPane!, { clientX: 2, clientY: 40, dataTransfer });

    expect(collectLeafIds(getLayout("s1"))).toEqual(["p2", "p1"]);
    expect(container.querySelector('[data-drop-side="left"]')).toBeNull();
  });
});
