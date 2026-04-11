import { writable, type Readable } from "svelte/store";
import type { SpawnProfile } from "$lib/panes/profiles";

/**
 * App-global "open the custom profile editor" pump. Exists because the
 * command palette's split-with-profile flow needs an inline editor just
 * like `NewSessionDialog` does, but a palette item can't mount a modal —
 * the palette is a flat list of actions. Instead, the action flips this
 * store, `App.svelte` renders the editor, and the promise from
 * `openCustomProfileEditor` resolves once the user submits or cancels.
 *
 * Only one editor can be open at a time. A second call while the editor
 * is already open will overwrite the pending resolver — the previous
 * call is implicitly cancelled. In practice the palette can't fire twice
 * without the modal having closed first, so this simplification is safe.
 */
interface ModalState {
  visible: boolean;
}

const state = writable<ModalState>({ visible: false });
let pendingResolve: ((profile: SpawnProfile | null) => void) | null = null;

export const customProfileModalState: Readable<ModalState> = { subscribe: state.subscribe };

/**
 * Open the editor and wait for the user's decision. Resolves to the
 * submitted profile, or `null` if the user cancelled / pressed Escape.
 */
export function openCustomProfileEditor(): Promise<SpawnProfile | null> {
  // Preempt any pending prior call so we don't strand its promise.
  pendingResolve?.(null);
  return new Promise((resolve) => {
    pendingResolve = resolve;
    state.set({ visible: true });
  });
}

/** Called by the hosting component when the user submits a profile. */
export function submitCustomProfile(profile: SpawnProfile): void {
  const resolve = pendingResolve;
  pendingResolve = null;
  state.set({ visible: false });
  resolve?.(profile);
}

/** Called by the hosting component when the user cancels or hits Escape. */
export function closeCustomProfileEditor(): void {
  const resolve = pendingResolve;
  pendingResolve = null;
  state.set({ visible: false });
  resolve?.(null);
}
