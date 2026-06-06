import { describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { WorkItem } from "$lib/bindings";
import {
  WORK_ITEM_DRAG_MIME,
  clearDraggedWorkItem,
  draggedWorkItem,
  hasWorkItemDragData,
  readWorkItemDragData,
  workItemDragPayload,
  writeWorkItemDragData,
} from "../drag";

function item(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: "wi-1",
    projectId: null,
    parentId: null,
    branch: null,
    fetchFirst: null,
    title: "Ship the board",
    body: null,
    status: "todo",
    repoPath: null,
    agentProfile: null,
    baseBranch: null,
    worktreePath: null,
    startError: null,
    sessionId: null,
    provider: null,
    externalId: null,
    externalUrl: null,
    sortOrder: 0,
    pinnedPrUrl: null,
    archivedAt: null,
    cost: null,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  } as WorkItem;
}

function dataTransfer() {
  const values = new Map<string, string>();
  return {
    effectAllowed: "uninitialized",
    dropEffect: "none",
    get types() {
      return Array.from(values.keys());
    },
    setData: vi.fn((type: string, value: string) => values.set(type, value)),
    getData: vi.fn((type: string) => values.get(type) ?? ""),
  } as unknown as DataTransfer;
}

describe("Work item drag data", () => {
  it("builds a payload from the item id and status", () => {
    expect(workItemDragPayload(item({ status: "doing" }))).toEqual({
      itemId: "wi-1",
      fromStatus: "doing",
    });
  });

  it("writes and reads drag metadata round-trip", () => {
    const transfer = dataTransfer();
    clearDraggedWorkItem();

    expect(writeWorkItemDragData(transfer, item())).toBe(true);

    expect(transfer.effectAllowed).toBe("move");
    expect(transfer.setData).toHaveBeenCalledWith(
      WORK_ITEM_DRAG_MIME,
      JSON.stringify({ itemId: "wi-1", fromStatus: "todo" }),
    );
    expect(readWorkItemDragData(transfer)).toEqual({
      itemId: "wi-1",
      fromStatus: "todo",
    });
    expect(hasWorkItemDragData(transfer)).toBe(true);
    expect(get(draggedWorkItem)).toEqual({
      itemId: "wi-1",
      fromStatus: "todo",
    });

    clearDraggedWorkItem();
    expect(get(draggedWorkItem)).toBeNull();
  });

  it("accepts Planning as a valid drag source column", () => {
    const transfer = dataTransfer();

    expect(writeWorkItemDragData(transfer, item({ status: "ready" }))).toBe(
      true,
    );
    expect(readWorkItemDragData(transfer)).toEqual({
      itemId: "wi-1",
      fromStatus: "ready",
    });
  });

  it("returns false / null for a missing DataTransfer", () => {
    expect(writeWorkItemDragData(null, item())).toBe(false);
    expect(hasWorkItemDragData(null)).toBe(false);
    expect(readWorkItemDragData(null)).toBeNull();
  });

  it("ignores malformed drag metadata", () => {
    const transfer = dataTransfer();
    transfer.setData(WORK_ITEM_DRAG_MIME, "{");
    expect(readWorkItemDragData(transfer)).toBeNull();

    const empty = dataTransfer();
    empty.setData(WORK_ITEM_DRAG_MIME, JSON.stringify({ fromStatus: "todo" }));
    expect(readWorkItemDragData(empty)).toBeNull();

    const unknownStatus = dataTransfer();
    unknownStatus.setData(
      WORK_ITEM_DRAG_MIME,
      JSON.stringify({ itemId: "wi-1", fromStatus: "backlog" }),
    );
    expect(readWorkItemDragData(unknownStatus)).toBeNull();
  });
});
