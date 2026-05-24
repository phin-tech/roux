import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  workItems,
  itemsByColumn,
  applyWorkItemEvent,
  WORK_ITEM_COLUMNS,
} from "../workItems";
import type { WorkItem } from "$lib/bindings";

function makeItem(overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id: crypto.randomUUID(),
    title: "Test item",
    status: "todo",
    sortOrder: 0,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

describe("workItems store", () => {
  beforeEach(() => {
    workItems.set([]);
  });

  describe("applyWorkItemEvent - created", () => {
    it("adds a new item", () => {
      const item = makeItem({ title: "new" });
      applyWorkItemEvent({ type: "created", item });
      expect(get(workItems)).toHaveLength(1);
      expect(get(workItems)[0].title).toBe("new");
    });

    it("dedupes on id", () => {
      const item = makeItem();
      applyWorkItemEvent({ type: "created", item });
      applyWorkItemEvent({ type: "created", item });
      expect(get(workItems)).toHaveLength(1);
    });
  });

  describe("applyWorkItemEvent - updated", () => {
    it("replaces by id", () => {
      const item = makeItem({ title: "original" });
      workItems.set([item]);
      const updated = { ...item, title: "updated" };
      applyWorkItemEvent({ type: "updated", item: updated });
      expect(get(workItems)[0].title).toBe("updated");
    });
  });

  describe("applyWorkItemEvent - moved", () => {
    it("updates status and sortOrder", () => {
      const item = makeItem({ status: "todo", sortOrder: 0 });
      workItems.set([item]);
      applyWorkItemEvent({ type: "moved", id: item.id, status: "doing", sortOrder: 1 });
      const result = get(workItems)[0];
      expect(result.status).toBe("doing");
      expect(result.sortOrder).toBe(1);
    });
  });

  describe("applyWorkItemEvent - deleted", () => {
    it("removes by id", () => {
      const a = makeItem();
      const b = makeItem();
      workItems.set([a, b]);
      applyWorkItemEvent({ type: "deleted", id: a.id });
      const list = get(workItems);
      expect(list).toHaveLength(1);
      expect(list[0].id).toBe(b.id);
    });
  });

  describe("applyWorkItemEvent - sessionBound", () => {
    it("binds sessionId to item", () => {
      const item = makeItem({ sessionId: null });
      workItems.set([item]);
      applyWorkItemEvent({ type: "sessionBound", id: item.id, sessionId: "sess-1" });
      expect(get(workItems)[0].sessionId).toBe("sess-1");
    });
  });

  describe("itemsByColumn derived store", () => {
    it("groups items by status", () => {
      const a = makeItem({ status: "todo", sortOrder: 0 });
      const b = makeItem({ status: "doing", sortOrder: 0 });
      const c = makeItem({ status: "todo", sortOrder: 1 });
      workItems.set([a, b, c]);
      const cols = get(itemsByColumn);
      expect(cols.get("todo")).toHaveLength(2);
      expect(cols.get("doing")).toHaveLength(1);
      expect(cols.get("review")).toHaveLength(0);
      expect(cols.get("done")).toHaveLength(0);
    });

    it("sorts by sortOrder within a column", () => {
      const a = makeItem({ status: "todo", sortOrder: 5 });
      const b = makeItem({ status: "todo", sortOrder: 2 });
      workItems.set([a, b]);
      const todos = get(itemsByColumn).get("todo")!;
      expect(todos[0].sortOrder).toBe(2);
      expect(todos[1].sortOrder).toBe(5);
    });

    it("all defined columns are present even when empty", () => {
      workItems.set([]);
      const cols = get(itemsByColumn);
      for (const col of WORK_ITEM_COLUMNS) {
        expect(cols.has(col)).toBe(true);
      }
    });
  });
});
