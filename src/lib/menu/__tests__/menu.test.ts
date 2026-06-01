import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Tauri menu mocks — every class factory returns a plain object with
// recording methods. The menu builder walks these like real instances.
// ---------------------------------------------------------------------------

type FakeItem = {
  __type: string;
  opts: Record<string, unknown>;
  items?: FakeItem[];
  setEnabled: ReturnType<typeof vi.fn>;
  setText: ReturnType<typeof vi.fn>;
  setChecked?: ReturnType<typeof vi.fn>;
  setAsAppMenu: ReturnType<typeof vi.fn>;
  setAsWindowMenu: ReturnType<typeof vi.fn>;
};

function makeClass(kind: string, hasChecked = false) {
  return {
    new: vi.fn(async (opts: Record<string, unknown> = {}) => {
      const self: FakeItem = {
        __type: kind,
        opts,
        items: Array.isArray(opts.items)
          ? (opts.items as FakeItem[])
          : undefined,
        setEnabled: vi.fn(async () => {}),
        setText: vi.fn(async (text: string) => {
          self.opts.text = text;
        }),
        setChecked: hasChecked ? vi.fn(async () => {}) : undefined,
        setAsAppMenu: vi.fn(async () => null),
        setAsWindowMenu: vi.fn(async () => null),
      };
      return self;
    }),
  };
}

vi.mock("@tauri-apps/api/menu/menu", () => ({ Menu: makeClass("Menu") }));
vi.mock("@tauri-apps/api/menu/submenu", () => ({ Submenu: makeClass("Submenu") }));
vi.mock("@tauri-apps/api/menu/menuItem", () => ({ MenuItem: makeClass("MenuItem") }));
vi.mock("@tauri-apps/api/menu/predefinedMenuItem", () => ({
  PredefinedMenuItem: makeClass("PredefinedMenuItem"),
}));
vi.mock("@tauri-apps/api/menu/checkMenuItem", () => ({
  CheckMenuItem: makeClass("CheckMenuItem", true),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
  initLogging: vi.fn(async () => {}),
}));

// Platform mock; each test sets the value it wants before dynamic import.
const platformMock = { isMacPlatform: vi.fn(() => true), hasPrimaryModifier: vi.fn(() => true), shortcutDisplayPart: vi.fn((s: string) => s), formatShortcut: vi.fn((s: string) => s) };
vi.mock("$lib/platform", () => platformMock);

// ---------------------------------------------------------------------------
// Walkers / assertions
// ---------------------------------------------------------------------------

function flatten(root: FakeItem): FakeItem[] {
  const out: FakeItem[] = [];
  const walk = (node: FakeItem) => {
    out.push(node);
    for (const child of node.items ?? []) walk(child);
  };
  walk(root);
  return out;
}

function findSubmenu(root: FakeItem, text: string): FakeItem | null {
  return (
    flatten(root).find(
      (n) => n.__type === "Submenu" && n.opts.text === text,
    ) ?? null
  );
}

function directChildSubmenus(root: FakeItem): FakeItem[] {
  return (root.items ?? []).filter((n) => n.__type === "Submenu");
}

function itemIds(submenu: FakeItem): string[] {
  return (submenu.items ?? [])
    .map((n) => (n.opts.id as string | undefined) ?? "")
    .filter(Boolean);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("setupAppMenu", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  afterEach(async () => {
    const { teardownAppMenu } = await import("../appMenu");
    teardownAppMenu();
    vi.clearAllMocks();
  });

  async function buildForPlatform(isMac: boolean): Promise<FakeItem> {
    platformMock.isMacPlatform.mockReturnValue(isMac);
    const { registerCommands } = await import("$lib/commands");
    registerCommands();
    const { setupAppMenu } = await import("../appMenu");
    await setupAppMenu(() => {});
    const { Menu } = (await import("@tauri-apps/api/menu/menu")) as unknown as {
      Menu: { new: ReturnType<typeof vi.fn> };
    };
    // Menu.new returned the last constructed menu; grab it from the
    // resolved value of the most recent call.
    const calls = Menu.new.mock.results;
    const lastResult = calls[calls.length - 1].value as Promise<FakeItem>;
    return await lastResult;
  }

  it("builds a macOS menu with a leading 'Roux' app submenu", async () => {
    const menu = await buildForPlatform(true);
    const topLevel = directChildSubmenus(menu).map((s) => s.opts.text);
    expect(topLevel[0]).toBe("Roux");
    expect(topLevel).toEqual([
      "Roux",
      "File",
      "Edit",
      "View",
      "Session",
      "Pane",
      "Tools",
      "Window",
      "Help",
    ]);
  });

  it("uses the native macOS Quit menu item", async () => {
    const menu = await buildForPlatform(true);
    const app = findSubmenu(menu, "Roux");
    expect(app).not.toBeNull();
    const items = app!.items ?? [];

    expect(itemIds(app!)).not.toContain("cmd:app.quit");
    expect(
      items.some((n) => n.__type === "PredefinedMenuItem" && n.opts.item === "Quit"),
    ).toBe(true);
  });

  it("relocates Settings, Check for Updates, Quit to File on non-mac", async () => {
    const menu = await buildForPlatform(false);
    const topLevel = directChildSubmenus(menu).map((s) => s.opts.text);
    expect(topLevel).not.toContain("Roux");
    const file = findSubmenu(menu, "File");
    expect(file).not.toBeNull();
    const ids = itemIds(file!);
    expect(ids).toContain("cmd:app.settings");
    expect(ids).toContain("cmd:app.check-updates");
    expect(ids).toContain("cmd:app.quit");
  });

  it("relocates About to Help on non-mac", async () => {
    const menu = await buildForPlatform(false);
    const help = findSubmenu(menu, "Help");
    expect(help).not.toBeNull();
    // PredefinedMenuItem About is passed as `{ item: { About: null } }`.
    const hasAbout = (help!.items ?? []).some(
      (n) =>
        n.__type === "PredefinedMenuItem" &&
        typeof n.opts.item === "object" &&
        n.opts.item !== null &&
        "About" in (n.opts.item as object),
    );
    expect(hasAbout).toBe(true);
  });

  it("keeps predefined Edit items across platforms", async () => {
    for (const isMac of [true, false]) {
      const menu = await buildForPlatform(isMac);
      const edit = findSubmenu(menu, "Edit");
      expect(edit).not.toBeNull();
      const predefinedKinds = (edit!.items ?? [])
        .filter((n) => n.__type === "PredefinedMenuItem")
        .map((n) => n.opts.item);
      expect(predefinedKinds).toContain("Undo");
      expect(predefinedKinds).toContain("Redo");
      expect(predefinedKinds).toContain("Cut");
      expect(predefinedKinds).toContain("Copy");
      expect(predefinedKinds).toContain("Paste");
      expect(predefinedKinds).toContain("SelectAll");
      const { teardownAppMenu } = await import("../appMenu");
      teardownAppMenu();
      vi.resetModules();
    }
  });

  it("exposes Group By as three CheckMenuItems under View", async () => {
    const menu = await buildForPlatform(true);
    const view = findSubmenu(menu, "View");
    expect(view).not.toBeNull();
    const groupBy = findSubmenu(view!, "Group Sessions By");
    expect(groupBy).not.toBeNull();
    const checks = (groupBy!.items ?? []).filter(
      (n) => n.__type === "CheckMenuItem",
    );
    expect(checks.length).toBe(3);
    expect(checks.map((c) => c.opts.text)).toEqual([
      "Repository",
      "Project",
      "Session (flat)",
    ]);
  });

  it("registers commands whose ids resolve against the command registry", async () => {
    await buildForPlatform(true);
    const { registry } = await import("$lib/commands");
    const { Menu } = (await import("@tauri-apps/api/menu/menu")) as unknown as {
      Menu: { new: ReturnType<typeof vi.fn> };
    };
    const menu = await (Menu.new.mock.results.at(-1)!.value as Promise<FakeItem>);

    const menuCommandIds = flatten(menu)
      .filter((n) => n.__type === "MenuItem")
      .map((n) => n.opts.id as string)
      .filter((id) => id?.startsWith("cmd:"))
      .map((id) => id.slice("cmd:".length));

    const unknown = menuCommandIds.filter((id) => !registry.get(id));
    expect(unknown).toEqual([]);
  });

  it("chooses setAsAppMenu on mac and setAsWindowMenu elsewhere", async () => {
    const macMenu = await buildForPlatform(true);
    expect(macMenu.setAsAppMenu).toHaveBeenCalled();
    expect(macMenu.setAsWindowMenu).not.toHaveBeenCalled();

    const { teardownAppMenu } = await import("../appMenu");
    teardownAppMenu();
    vi.resetModules();

    const winMenu = await buildForPlatform(false);
    expect(winMenu.setAsWindowMenu).toHaveBeenCalled();
    expect(winMenu.setAsAppMenu).not.toHaveBeenCalled();
  });
});

describe("claimFire dedup", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("returns true first, false within the window, true after gap", async () => {
    const { claimFire, __test } = await import("../appMenu");
    __test.resetDedup();

    expect(claimFire("CmdOrCtrl+N")).toBe(true);
    expect(claimFire("CmdOrCtrl+N")).toBe(false);
    expect(claimFire("CmdOrCtrl+S")).toBe(true);

    // Advance past the 80ms dedup window.
    await new Promise((r) => setTimeout(r, 120));
    expect(claimFire("CmdOrCtrl+N")).toBe(true);
  });
});
