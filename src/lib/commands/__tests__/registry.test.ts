import { describe, it, expect, beforeEach } from "vitest";
import { CommandRegistry, type Command } from "../registry";

describe("CommandRegistry", () => {
  let registry: CommandRegistry;

  beforeEach(() => {
    registry = new CommandRegistry();
  });

  it("registers and retrieves commands", () => {
    registry.register({
      id: "test.cmd",
      label: "Test Command",
      category: "Test",
      execute: () => {},
    });

    const available = registry.getAvailable();
    expect(available).toHaveLength(1);
    expect(available[0].id).toBe("test.cmd");
  });

  it("unregisters commands", () => {
    registry.register({
      id: "test.cmd",
      label: "Test",
      category: "Test",
    });

    registry.unregister("test.cmd");
    expect(registry.getAvailable()).toHaveLength(0);
  });

  it("filters by availability", () => {
    registry.register({
      id: "always",
      label: "Always Available",
      category: "Test",
    });

    registry.register({
      id: "conditional",
      label: "Conditional",
      category: "Test",
      available: () => false,
    });

    const available = registry.getAvailable();
    expect(available).toHaveLength(1);
    expect(available[0].id).toBe("always");
  });

  it("commands without available() are always available", () => {
    registry.register({
      id: "no-check",
      label: "No Check",
      category: "Test",
    });

    expect(registry.getAvailable()).toHaveLength(1);
  });

  it("finds commands by shortcut", () => {
    registry.register({
      id: "test.cmd",
      label: "Test",
      shortcut: "cmd+k",
      category: "Test",
    });

    const found = registry.getByShortcut("cmd+k");
    expect(found).toBeDefined();
    expect(found!.id).toBe("test.cmd");
  });

  it("returns undefined for unknown shortcut", () => {
    expect(registry.getByShortcut("cmd+z")).toBeUndefined();
  });

  it("executes a command", () => {
    let executed = false;
    registry.register({
      id: "test.cmd",
      label: "Test",
      category: "Test",
      execute: () => { executed = true; },
    });

    registry.execute("test.cmd");
    expect(executed).toBe(true);
  });

  it("handles executing non-existent command gracefully", () => {
    expect(() => registry.execute("nonexistent")).not.toThrow();
  });

  it("handles executing command without execute fn", () => {
    registry.register({
      id: "no-exec",
      label: "No Execute",
      category: "Test",
    });

    expect(() => registry.execute("no-exec")).not.toThrow();
  });

  it("supports multi-step commands with getItems", async () => {
    registry.register({
      id: "multi",
      label: "Multi Step",
      category: "Test",
      getItems: async () => [
        { id: "item-1", label: "Item 1", action: () => {} },
        { id: "item-2", label: "Item 2", action: () => {} },
      ],
    });

    const cmd = registry.getAvailable()[0];
    expect(cmd.getItems).toBeDefined();
    const items = await cmd.getItems!();
    expect(items).toHaveLength(2);
    expect(items[0].label).toBe("Item 1");
  });

  it("supports items with substeps for drill-in", async () => {
    registry.register({
      id: "drill",
      label: "Drill",
      category: "Test",
      getItems: async () => [
        {
          id: "parent",
          label: "Parent",
          substeps: () => [
            { id: "child-1", label: "Child 1", action: () => {} },
            { id: "child-2", label: "Child 2", action: () => {} },
          ],
        },
      ],
    });

    const items = await registry.getAvailable()[0].getItems!();
    const substeps = await items[0].substeps!();
    expect(substeps).toHaveLength(2);
    expect(substeps[0].label).toBe("Child 1");
  });

  it("groups commands by category", () => {
    registry.register({ id: "a", label: "A", category: "Alpha" });
    registry.register({ id: "b", label: "B", category: "Beta" });
    registry.register({ id: "c", label: "C", category: "Alpha" });

    const available = registry.getAvailable();
    const alphas = available.filter(c => c.category === "Alpha");
    const betas = available.filter(c => c.category === "Beta");

    expect(alphas).toHaveLength(2);
    expect(betas).toHaveLength(1);
  });
});
