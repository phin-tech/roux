// Global detection state for the worktrunk CLI. Refreshed once at launch
// (and whenever the override setting changes); the sidebar activity rail
// subscribes so the Worktrunk icon only renders when `wt` is installed.

import { writable, type Readable } from "svelte/store";
import { commands } from "$lib/bindings";

export interface WorktrunkDetectionState {
  /** Resolved binary path, `null` when wt isn't installed or below min version. */
  binaryPath: string | null;
  /** Parsed version string when `binaryPath` is set. */
  version: string | null;
  /**
   * `true` once we've completed at least one probe (even if it returned
   * null). Distinct from "installed" so consumers can render a loading
   * skeleton during the initial probe rather than flashing "not installed".
   */
  probed: boolean;
}

const state = writable<WorktrunkDetectionState>({
  binaryPath: null,
  version: null,
  probed: false,
});

export const worktrunkDetection: Readable<WorktrunkDetectionState> = {
  subscribe: state.subscribe,
};

export async function refreshWorktrunkDetection(): Promise<void> {
  try {
    const result = await commands.cmdDetectWorktrunk(null);
    state.set({
      binaryPath: result.binaryPath,
      version: result.version,
      probed: true,
    });
  } catch {
    state.set({ binaryPath: null, version: null, probed: true });
  }
}

/**
 * Hard reset for tests.
 */
export function _resetWorktrunkDetectionForTests(): void {
  state.set({ binaryPath: null, version: null, probed: false });
}

/**
 * Snapshot accessor — non-reactive, for one-off checks outside Svelte
 * reactive contexts.
 */
export function isWorktrunkInstalledSnapshot(): boolean {
  let installed = false;
  state.subscribe((s) => {
    installed = s.binaryPath !== null;
  })();
  return installed;
}

/**
 * Test-only setter. Production code must go through
 * [`refreshWorktrunkDetection`].
 */
export function _setWorktrunkDetectionForTests(
  next: WorktrunkDetectionState,
): void {
  state.set(next);
}
