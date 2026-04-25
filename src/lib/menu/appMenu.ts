// Native menu bar setup.
//
// Builds the app's top-level menu from the existing command registry and
// keymap, installs it as the macOS app menu or the main window's menu bar
// on Windows/Linux, and keeps its enabled/checked state in sync with the
// reactive stores that already drive `available()` for each command.
//
// Menu clicks route through a caller-supplied dispatch function (normally
// App.svelte's `executeCommandById`) so prompt flows, the quit dialog, and
// command-palette overrides continue to work unchanged.

import { Menu } from "@tauri-apps/api/menu/menu";
import { Submenu } from "@tauri-apps/api/menu/submenu";
import { MenuItem } from "@tauri-apps/api/menu/menuItem";
import { PredefinedMenuItem } from "@tauri-apps/api/menu/predefinedMenuItem";
import { CheckMenuItem } from "@tauri-apps/api/menu/checkMenuItem";
import { get, type Unsubscriber } from "svelte/store";
import { isMacPlatform } from "$lib/platform";
import { registry } from "$lib/commands";
import { keymapState, shortcutFor } from "$lib/keymap/store";
import { sessionState } from "$lib/stores/sessions";
import { paneInstances } from "$lib/panes/instances";
import { focusedPaneId } from "$lib/panes/focus";
import { paneSlotById } from "$lib/stores/ui";
import { settings, updateSetting } from "$lib/stores/settings";
import { openCommandPaletteWithCommand } from "$lib/stores/commandSurface";
import { toTauriAccelerator } from "./accelerators";
import { logError } from "$lib/logging";

// ---------------------------------------------------------------------------
// Double-fire dedup
// ---------------------------------------------------------------------------

const DEDUP_MS = 80;
const recentlyFired = new Map<string, number>();

/**
 * Returns true on the first call for an accelerator within `DEDUP_MS`,
 * false on subsequent calls within the window. Used to prevent the OS-level
 * menu shortcut and the webview's own keymap dispatcher from both firing
 * the same command when both handle the same chord.
 *
 * Cross-platform behavior: on macOS the OS menu handler tends to arrive
 * before the webview keydown; on Windows/Linux ordering is less
 * predictable. Because both sides call `claimFire` before dispatching,
 * whichever arrives first wins and the second is dropped regardless of
 * order.
 */
export function claimFire(accelerator: string): boolean {
  const now = performance.now();
  const prev = recentlyFired.get(accelerator);
  if (prev !== undefined && now - prev < DEDUP_MS) return false;
  recentlyFired.set(accelerator, now);
  if (recentlyFired.size > 64) {
    for (const [k, t] of recentlyFired) {
      if (now - t > DEDUP_MS) recentlyFired.delete(k);
    }
  }
  return true;
}

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

export type MenuDispatch = (commandId: string) => void;

interface TrackedItem {
  commandId: string;
  item: MenuItem | CheckMenuItem;
  /**
   * Optional label resolver. When present, the refresh pass calls it and
   * pushes the result through `setText`. Used by the Pane > Focus slots so
   * a renamed pane shows its current name instead of a bare "Pane N".
   */
  refreshText?: () => string;
}

let disposers: Unsubscriber[] = [];
let tracked: TrackedItem[] = [];
let groupByRepoItem: CheckMenuItem | null = null;
let groupByProjectItem: CheckMenuItem | null = null;
let rebuildTimer: ReturnType<typeof setTimeout> | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let currentDispatch: MenuDispatch | null = null;

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/**
 * Install the native menu and wire store subscriptions that keep it
 * current. Safe to call once per window; call `teardownAppMenu` before
 * calling again. Runs during App.svelte `onMount` after both the command
 * registry and the keymap are loaded — accelerators are read from
 * `shortcutFor` at build time.
 */
export async function setupAppMenu(dispatch: MenuDispatch): Promise<void> {
  currentDispatch = dispatch;
  await rebuildMenu();

  // Rebuild whenever the keymap changes so accelerators follow the active
  // preset. Initial subscribe fires immediately — skip that one.
  let firstKeymap = true;
  disposers.push(
    keymapState.subscribe(() => {
      if (firstKeymap) {
        firstKeymap = false;
        return;
      }
      scheduleRebuild();
    }),
  );

  // Enablement depends on these stores. Coalesce per-frame refreshes so
  // bursty updates (e.g. session restore on startup) don't hammer Tauri
  // with per-item setEnabled calls.
  const refresh = () => scheduleRefresh();
  disposers.push(sessionState.subscribe(refresh));
  disposers.push(paneInstances.subscribe(refresh));
  disposers.push(focusedPaneId.subscribe(refresh));
  disposers.push(settings.subscribe(refresh));
}

export function teardownAppMenu(): void {
  if (rebuildTimer) {
    clearTimeout(rebuildTimer);
    rebuildTimer = null;
  }
  if (refreshTimer) {
    clearTimeout(refreshTimer);
    refreshTimer = null;
  }
  for (const d of disposers) d();
  disposers = [];
  tracked = [];
  groupByRepoItem = null;
  groupByProjectItem = null;
  currentDispatch = null;
}

function scheduleRebuild(): void {
  if (rebuildTimer) clearTimeout(rebuildTimer);
  rebuildTimer = setTimeout(() => {
    rebuildTimer = null;
    void rebuildMenu();
  }, 50);
}

function scheduleRefresh(): void {
  if (refreshTimer) return;
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    refreshEnabled();
  }, 0);
}

async function rebuildMenu(): Promise<void> {
  if (!currentDispatch) return;
  const dispatch = currentDispatch;
  try {
    // Build into fresh buffers; only swap on success so a half-built menu
    // can't leave stale references in `tracked`.
    const nextTracked: TrackedItem[] = [];
    const nextGroupBy = { repo: null as CheckMenuItem | null, project: null as CheckMenuItem | null };
    const menu = await buildMenu({
      dispatch,
      isMac: isMacPlatform(),
      track: (commandId, item, refreshText) =>
        nextTracked.push({ commandId, item, refreshText }),
      trackGroupBy: (repo, project) => {
        nextGroupBy.repo = repo;
        nextGroupBy.project = project;
      },
    });
    tracked = nextTracked;
    groupByRepoItem = nextGroupBy.repo;
    groupByProjectItem = nextGroupBy.project;
    if (isMacPlatform()) {
      await menu.setAsAppMenu();
    } else {
      await menu.setAsWindowMenu();
    }
    refreshEnabled();
  } catch (e) {
    logError("appMenu: rebuild failed", e);
  }
}

function refreshEnabled(): void {
  for (const entry of tracked) {
    // Guard per-entry so one failing item can't short-circuit the rest of
    // the refresh — e.g. a future Tauri version removing setText or
    // setEnabled on some item variant.
    try {
      const cmd = registry.get(entry.commandId);
      // Predefined items skipped at track time; every tracked id should
      // be in the registry. If not, treat as always-enabled — matches
      // the keymap-pseudo-command case.
      const enabled = !cmd || !cmd.available || cmd.available();
      void entry.item.setEnabled(enabled).catch(() => {});
      if (entry.refreshText) {
        const text = entry.refreshText();
        // MenuItem.setText exists on both MenuItem and CheckMenuItem.
        void (entry.item as MenuItem).setText(text).catch(() => {});
      }
    } catch (e) {
      logError(`appMenu: refresh failed for ${entry.commandId}`, e);
    }
  }
  const groupBy = get(settings).groupBy ?? "repo";
  if (groupByRepoItem) {
    void groupByRepoItem.setChecked(groupBy === "repo").catch(() => {});
  }
  if (groupByProjectItem) {
    void groupByProjectItem.setChecked(groupBy !== "repo").catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// Menu construction
// ---------------------------------------------------------------------------

interface BuildContext {
  dispatch: MenuDispatch;
  isMac: boolean;
  track: (
    commandId: string,
    item: MenuItem | CheckMenuItem,
    refreshText?: () => string,
  ) => void;
  trackGroupBy: (repo: CheckMenuItem, project: CheckMenuItem) => void;
}

async function buildMenu(ctx: BuildContext): Promise<Menu> {
  const submenus: Submenu[] = [];
  if (ctx.isMac) submenus.push(await buildAppMenu(ctx));
  submenus.push(await buildFileMenu(ctx));
  submenus.push(await buildEditMenu());
  submenus.push(await buildViewMenu(ctx));
  submenus.push(await buildSessionMenu(ctx));
  submenus.push(await buildPaneMenu(ctx));
  submenus.push(await buildToolsMenu(ctx));
  submenus.push(await buildWindowMenu());
  submenus.push(await buildHelpMenu(ctx));
  return Menu.new({ items: submenus });
}

async function buildAppMenu(ctx: BuildContext): Promise<Submenu> {
  return Submenu.new({
    text: "Roux",
    items: [
      await PredefinedMenuItem.new({ item: { About: null } }),
      await sep(),
      await cmdItem(ctx, "app.settings", "Settings\u2026"),
      await cmdItem(ctx, "app.check-updates", "Check for Updates\u2026"),
      await sep(),
      await PredefinedMenuItem.new({ item: "Services" }),
      await sep(),
      await PredefinedMenuItem.new({ item: "Hide" }),
      await PredefinedMenuItem.new({ item: "HideOthers" }),
      await PredefinedMenuItem.new({ item: "ShowAll" }),
      await sep(),
      await cmdItem(ctx, "app.quit", "Quit Roux"),
    ],
  });
}

async function buildFileMenu(ctx: BuildContext): Promise<Submenu> {
  const items: Array<MenuItem | Submenu | PredefinedMenuItem> = [
    await cmdItem(ctx, "session.new", "New Session"),
    await Submenu.new({
      text: "New Worktree",
      items: [
        await cmdItem(ctx, "session.new-worktree-from-current", "From Current Branch"),
        await cmdItem(ctx, "session.new-worktree-from-main", "From Main"),
        await cmdItem(ctx, "session.new-worktree-from-origin-main", "From origin/main"),
      ],
    }),
    await sep(),
    await cmdItem(ctx, "pane.close", "Close Pane"),
    await cmdItem(ctx, "session.close", "Close Session"),
  ];
  if (!ctx.isMac) {
    items.push(
      await sep(),
      await cmdItem(ctx, "app.settings", "Settings\u2026"),
      await cmdItem(ctx, "app.check-updates", "Check for Updates\u2026"),
      await sep(),
      await cmdItem(ctx, "app.quit", "Exit"),
    );
  }
  return Submenu.new({ text: "File", items });
}

async function buildEditMenu(): Promise<Submenu> {
  return Submenu.new({
    text: "Edit",
    items: [
      await PredefinedMenuItem.new({ item: "Undo" }),
      await PredefinedMenuItem.new({ item: "Redo" }),
      await sep(),
      await PredefinedMenuItem.new({ item: "Cut" }),
      await PredefinedMenuItem.new({ item: "Copy" }),
      await PredefinedMenuItem.new({ item: "Paste" }),
      await PredefinedMenuItem.new({ item: "SelectAll" }),
    ],
  });
}

async function buildViewMenu(ctx: BuildContext): Promise<Submenu> {
  const groupBy = get(settings).groupBy ?? "repo";
  const repoItem = await CheckMenuItem.new({
    id: "menu:groupBy:repo",
    text: "Repository",
    checked: groupBy === "repo",
    action: () => {
      // Direct setting flip — bypasses the command registry because
      // `ui.group-by` is a getItems picker with no execute, and the menu
      // needs a direct toggle per CheckMenuItem.
      updateSetting("groupBy", "repo");
    },
  });
  const projectItem = await CheckMenuItem.new({
    id: "menu:groupBy:project",
    text: "Project",
    checked: groupBy !== "repo",
    action: () => {
      updateSetting("groupBy", "project");
    },
  });
  ctx.trackGroupBy(repoItem, projectItem);

  return Submenu.new({
    text: "View",
    items: [
      await cmdItem(ctx, "ui.toggle-sidebar", "Toggle Sidebar"),
      await cmdItem(ctx, "ui.toggle-notes", "Toggle Notes"),
      await cmdItem(ctx, "ui.toggle-watches", "Toggle Watches"),
      await cmdItem(ctx, "ui.toggle-notifications", "Toggle Notifications"),
      await cmdItem(ctx, "ui.toggle-task-panel", "Toggle Task Panel"),
      await sep(),
      await Submenu.new({
        text: "Group Sessions By",
        items: [repoItem, projectItem],
      }),
      await sep(),
      await PredefinedMenuItem.new({ item: "Fullscreen" }),
    ],
  });
}

async function buildSessionMenu(ctx: BuildContext): Promise<Submenu> {
  const focusItems: MenuItem[] = [];
  for (let i = 1; i <= 10; i++) {
    focusItems.push(await cmdItem(ctx, `session.focus-index-${i}`, `Session ${i}`));
  }
  return Submenu.new({
    text: "Session",
    items: [
      await cmdItem(ctx, "session.next", "Next Session"),
      await cmdItem(ctx, "session.prev", "Previous Session"),
      await Submenu.new({ text: "Focus Session", items: focusItems }),
      await sep(),
      await cmdItem(ctx, "session.rename", "Rename Session\u2026"),
      await cmdItem(ctx, "session.open-in-editor", "Open in Editor"),
      await cmdItem(ctx, "session.reconnect", "Reconnect Session"),
      await cmdItem(ctx, "session.set-project", "Set Project\u2026"),
    ],
  });
}

async function buildPaneMenu(ctx: BuildContext): Promise<Submenu> {
  const focusItems: Array<MenuItem | PredefinedMenuItem> = [
    await cmdItem(ctx, "pane.focus-left", "Left"),
    await cmdItem(ctx, "pane.focus-right", "Right"),
    await cmdItem(ctx, "pane.focus-up", "Up"),
    await cmdItem(ctx, "pane.focus-down", "Down"),
    await cmdItem(ctx, "pane.focus-next", "Next"),
    await sep(),
  ];
  for (let i = 1; i <= 10; i++) {
    focusItems.push(await focusPaneItem(ctx, i));
  }
  const moveItems = [
    await cmdItem(ctx, "pane.move-left", "Left"),
    await cmdItem(ctx, "pane.move-right", "Right"),
    await cmdItem(ctx, "pane.move-up", "Up"),
    await cmdItem(ctx, "pane.move-down", "Down"),
  ];
  const resizeItems = [
    await cmdItem(ctx, "pane.resize-left", "Shrink Left"),
    await cmdItem(ctx, "pane.resize-right", "Grow Right"),
    await cmdItem(ctx, "pane.resize-up", "Grow Up"),
    await cmdItem(ctx, "pane.resize-down", "Shrink Down"),
  ];
  const splitWithProfileItems = [
    await cmdItem(ctx, "pane.split-claude", "Claude"),
    await cmdItem(ctx, "pane.split-codex", "Codex"),
    await sep(),
    await cmdItem(ctx, "pane.split-horizontal-with-profile", "Split Right with Profile\u2026"),
    await cmdItem(ctx, "pane.split-vertical-with-profile", "Split Down with Profile\u2026"),
  ];

  return Submenu.new({
    text: "Pane",
    items: [
      await cmdItem(ctx, "pane.split-horizontal", "Split Right"),
      await cmdItem(ctx, "pane.split-vertical", "Split Down"),
      await Submenu.new({ text: "Split with Profile", items: splitWithProfileItems }),
      await sep(),
      await Submenu.new({ text: "Focus", items: focusItems }),
      await Submenu.new({ text: "Move", items: moveItems }),
      await Submenu.new({ text: "Resize", items: resizeItems }),
      await sep(),
      await cmdItem(ctx, "pane.toggle-fullscreen", "Toggle Fullscreen"),
      await cmdItem(ctx, "pane.toggle-stack", "Toggle Stack"),
      await cmdItem(ctx, "pane.rename", "Rename Pane\u2026"),
      await sep(),
      await cmdItem(ctx, "pane.open-doc", "Open Doc\u2026"),
      await cmdItem(ctx, "pane.run-command", "Run Command\u2026"),
      await cmdItem(ctx, "pane.attach-terminal", "Attach Terminal\u2026"),
      await cmdItem(ctx, "pane.kill-terminal", "Kill Terminal"),
    ],
  });
}

async function buildToolsMenu(ctx: BuildContext): Promise<Submenu> {
  const watchItems = [
    await cmdItem(ctx, "watch.add-http", "HTTP Health Check\u2026"),
    await cmdItem(ctx, "watch.add-shell", "Shell Command\u2026"),
    await cmdItem(ctx, "watch.add-github", "GitHub Action\u2026"),
    await cmdItem(ctx, "watch.add-github-pr", "GitHub Pull Request\u2026"),
  ];
  return Submenu.new({
    text: "Tools",
    items: [
      await cmdItem(ctx, "app.command-palette", "Command Palette\u2026"),
      await cmdItem(ctx, "app.leader-mode", "Leader Mode"),
      await sep(),
      await cmdItem(ctx, "task.run", "Run Task\u2026"),
      await cmdItem(ctx, "task.rerun", "Rerun Command\u2026"),
      await sep(),
      await Submenu.new({ text: "Add Watch", items: watchItems }),
      await sep(),
      await cmdItem(ctx, "keymap.reload", "Reload Keymap"),
      await cmdItem(ctx, "keymap.open-in-editor", "Open Keybindings in Editor"),
      await cmdItem(ctx, "keymap.reset-to-default", "Reset Keybindings to Default…"),
    ],
  });
}

async function buildWindowMenu(): Promise<Submenu> {
  return Submenu.new({
    text: "Window",
    items: [await PredefinedMenuItem.new({ item: "Minimize" })],
  });
}

async function buildHelpMenu(ctx: BuildContext): Promise<Submenu> {
  const items: Array<MenuItem | PredefinedMenuItem> = [
    await cmdItem(ctx, "help.open-docs", "Roux Documentation"),
    await cmdItem(ctx, "help.report-issue", "Report an Issue"),
  ];
  if (!ctx.isMac) {
    items.push(await sep(), await PredefinedMenuItem.new({ item: { About: null } }));
  }
  return Submenu.new({ text: "Help", items });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function sep(): Promise<PredefinedMenuItem> {
  return PredefinedMenuItem.new({ item: "Separator" });
}

async function cmdItem(
  ctx: BuildContext,
  commandId: string,
  text: string,
): Promise<MenuItem> {
  const accelerator = toTauriAccelerator(shortcutFor(commandId)) ?? undefined;
  const cmd = registry.get(commandId);
  const initiallyEnabled = !cmd || !cmd.available || cmd.available();
  const item = await MenuItem.new({
    id: `cmd:${commandId}`,
    text,
    accelerator,
    enabled: initiallyEnabled,
    action: () => {
      // Dedup against any in-flight keymap dispatch for the same chord.
      if (accelerator && !claimFire(accelerator)) return;
      fireCommand(ctx.dispatch, commandId);
    },
  });
  ctx.track(commandId, item);
  return item;
}

async function focusPaneItem(ctx: BuildContext, slot: number): Promise<MenuItem> {
  const commandId = `pane.focus-index-${slot}`;
  const accelerator = toTauriAccelerator(shortcutFor(commandId)) ?? undefined;
  const resolveText = () => paneFocusLabel(slot);
  const item = await MenuItem.new({
    id: `cmd:${commandId}`,
    text: resolveText(),
    accelerator,
    enabled: !!paneAtSlot(slot),
    action: () => {
      if (accelerator && !claimFire(accelerator)) return;
      fireCommand(ctx.dispatch, commandId);
    },
  });
  ctx.track(commandId, item, resolveText);
  return item;
}

function paneAtSlot(slot: number): string | null {
  const slots = get(paneSlotById);
  for (const [paneId, s] of slots) {
    if (s === slot) return paneId;
  }
  return null;
}

function paneFocusLabel(slot: number): string {
  const paneId = paneAtSlot(slot);
  const base = `Pane ${slot}`;
  if (!paneId) return base;
  const name = get(paneInstances).get(paneId)?.name?.trim();
  if (!name) return base;
  return `${base}: ${name}`;
}

function fireCommand(dispatch: MenuDispatch, commandId: string): void {
  // getItems-only pickers (task.run, pane.open-doc, pane.attach-terminal,
  // session.set-project) have no `execute` or `onInput`. `dispatch` would
  // no-op. Open the command palette drilled into the picker instead so the
  // user can complete the action they just selected.
  const cmd = registry.get(commandId);
  if (cmd && cmd.getItems && !cmd.execute && !cmd.onInput) {
    openCommandPaletteWithCommand(commandId);
    return;
  }
  dispatch(commandId);
}

// ---------------------------------------------------------------------------
// Test-only accessors
// ---------------------------------------------------------------------------

/** @internal */
export const __test = {
  claimFire,
  resetDedup: () => recentlyFired.clear(),
};
