import { writable, get } from "svelte/store";

// Use `any` for xterm types to keep this module importable in test environments
// without pulling in the full xterm bundle.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Terminal = any;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type FitAddon = any;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type OutputChannel = any;

export type PaneType = "claude" | "shell" | "markdown" | "command";

export type CommandStatus = "idle" | "running" | "success" | "error";

export interface PaneInstance {
  id: string;
  type: PaneType;
  ptyId: string;

  // xterm state — null until attachToContainer is called
  terminal: Terminal | null;
  fitAddon: FitAddon | null;
  outputChannel: OutputChannel | null;

  // Cleanup hooks
  unlisteners: Array<() => void>;

  // Optional metadata
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;

  // Command-pane runtime state
  commandStatus?: CommandStatus;
  commandExitCode?: number | null;
  commandStartedAt?: number | null;
  elapsedTimer?: ReturnType<typeof setInterval> | null;
}

export interface CreatePaneOpts {
  id?: string;
  type: PaneType;
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
}

// ── Store ──────────────────────────────────────────────────

export const paneInstances = writable<Map<string, PaneInstance>>(new Map());

// ── Helpers ────────────────────────────────────────────────

function generateId(): string {
  return `pane-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

// ── Public API ─────────────────────────────────────────────

/**
 * Create a new pane instance and add it to the store.
 * Terminal creation is deferred — terminal is kept null until
 * attachToContainer is called.
 * Returns the pane id.
 */
export function createPane(opts: CreatePaneOpts): string {
  const id = opts.id ?? generateId();
  const instance: PaneInstance = {
    id,
    type: opts.type,
    ptyId: opts.ptyId,
    terminal: null,
    fitAddon: null,
    outputChannel: null,
    unlisteners: [],
    name: opts.name,
    workingDir: opts.workingDir,
    command: opts.command,
    docPath: opts.docPath,
    commandStatus: "idle",
    commandExitCode: null,
    commandStartedAt: null,
    elapsedTimer: null,
  };
  paneInstances.update((map) => {
    const next = new Map(map);
    next.set(id, instance);
    return next;
  });
  return id;
}

/**
 * Dispose a pane instance — idempotent.
 * Cleans up unlisteners, clears elapsed timer, disposes terminal, kills PTY
 * for shell/command types, and removes from the store.
 */
export function disposePane(id: string, killPty?: (ptyId: string) => Promise<void>): void {
  const map = get(paneInstances);
  const inst = map.get(id);
  if (!inst) return;

  // Kill PTY for shell/command panes (claude PTYs are session-level, killed separately)
  if (killPty && (inst.type === "shell" || inst.type === "command")) {
    killPty(inst.ptyId).catch(() => { /* best-effort */ });
  }

  // Run all cleanup listeners
  for (const unlisten of inst.unlisteners.splice(0)) {
    try {
      unlisten();
    } catch { /* best-effort */ }
  }

  // Clear command elapsed timer
  if (inst.elapsedTimer != null) {
    clearInterval(inst.elapsedTimer);
    inst.elapsedTimer = null;
  }

  // Dispose xterm terminal if one exists
  if (inst.terminal != null) {
    try {
      inst.terminal.dispose();
    } catch { /* best-effort */ }
    inst.terminal = null;
    inst.fitAddon = null;
    inst.outputChannel = null;
  }

  paneInstances.update((m) => {
    const next = new Map(m);
    next.delete(id);
    return next;
  });
}

/**
 * Tear down old PTY listeners on a pane and update its ptyId.
 * The caller is responsible for attaching new listeners.
 */
export function replacePty(paneId: string, newPtyId: string): void {
  paneInstances.update((map) => {
    const inst = map.get(paneId);
    if (!inst) return map;

    // Clean up old listeners
    for (const unlisten of inst.unlisteners.splice(0)) {
      try {
        unlisten();
      } catch { /* best-effort */ }
    }
    inst.outputChannel = null;

    const next = new Map(map);
    next.set(paneId, { ...inst, ptyId: newPtyId, unlisteners: [] });
    return next;
  });
}

/**
 * Partial update for metadata / status fields.
 */
export function updateInstance(
  paneId: string,
  fields: Partial<Omit<PaneInstance, "id">>
): void {
  paneInstances.update((map) => {
    const inst = map.get(paneId);
    if (!inst) return map;
    const next = new Map(map);
    next.set(paneId, { ...inst, ...fields });
    return next;
  });
}

/**
 * Read a single instance from the store synchronously.
 */
export function getInstance(paneId: string): PaneInstance | undefined {
  return get(paneInstances).get(paneId);
}

/**
 * Attach the terminal's DOM element to a container.
 * Suppresses WebKit focus theft on first open by briefly blurring after open.
 */
export function attachToContainer(
  paneId: string,
  container: HTMLElement
): void {
  const inst = getInstance(paneId);
  if (!inst?.terminal) return;
  inst.terminal.open(container);
  // Suppress WebKit focus theft: blur immediately after open
  try {
    (container.querySelector(".xterm-helper-textarea") as HTMLElement | null)
      ?.blur();
  } catch { /* best-effort */ }
  inst.fitAddon?.fit?.();
}

/**
 * Detach the terminal DOM element from its container.
 * xterm doesn't have a first-class "detach" API; we simply remove the
 * rendered children so the element can be re-parented later.
 */
export function detachFromContainer(paneId: string): void {
  const inst = getInstance(paneId);
  if (!inst?.terminal) return;
  const el: HTMLElement | undefined = inst.terminal.element;
  if (el?.parentElement) {
    el.parentElement.removeChild(el);
  }
}

/**
 * Reset the store to an empty map — only intended for tests.
 */
export function resetInstances(): void {
  // Dispose all existing instances cleanly
  const map = get(paneInstances);
  for (const id of map.keys()) {
    disposePane(id);
  }
  paneInstances.set(new Map());
}
