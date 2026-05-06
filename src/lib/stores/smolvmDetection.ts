// Global detection state for the smolvm CLI. Refreshed once at launch
// (and whenever the override setting changes); the activity rail
// subscribes so the smol-machines icon only renders when `smolvm` is
// installed.
//
// Pattern-mirrors `worktrunkDetection.ts`. The same `binaryPath !== null`
// signal gates the rail icon, the sidebar panel content, and any
// session-row badges that surface a smol-machine binding.

import { writable, type Readable } from "svelte/store";
import { commands } from "$lib/bindings";

export interface SmolvmDetectionState {
  /** Resolved binary path, `null` when smolvm isn't installed. */
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

const state = writable<SmolvmDetectionState>({
  binaryPath: null,
  version: null,
  probed: false,
});

export const smolvmDetection: Readable<SmolvmDetectionState> = {
  subscribe: state.subscribe,
};

export async function refreshSmolvmDetection(): Promise<void> {
  try {
    const result = await commands.cmdDetectSmolvm();
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
export function _resetSmolvmDetectionForTests(): void {
  state.set({ binaryPath: null, version: null, probed: false });
}

/**
 * Snapshot accessor — non-reactive, for one-off checks outside Svelte
 * reactive contexts.
 */
export function isSmolvmInstalledSnapshot(): boolean {
  let installed = false;
  state.subscribe((s) => {
    installed = s.binaryPath !== null;
  })();
  return installed;
}

/**
 * Test-only setter. Production code must go through
 * [`refreshSmolvmDetection`].
 */
export function _setSmolvmDetectionForTests(
  next: SmolvmDetectionState,
): void {
  state.set(next);
}
