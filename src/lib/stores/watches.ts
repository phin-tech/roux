import { writable, derived } from "svelte/store";
import type { Watch, WatchOutcome } from "$lib/types";

export const watchState = writable<Watch[]>([]);

export function addOrUpdateWatch(watch: Watch): void {
  watchState.update((watches) => {
    const idx = watches.findIndex((w) => w.id === watch.id);
    if (idx >= 0) {
      const updated = [...watches];
      updated[idx] = watch;
      return updated;
    }
    return [...watches, watch];
  });
}

export function removeWatchFromStore(id: string): void {
  watchState.update((watches) => watches.filter((w) => w.id !== id));
}

export function watchesForSession(sessionId: string) {
  return derived(watchState, ($watches) =>
    $watches.filter(
      (w) => w.scope.type === "session" && w.scope.sessionId === sessionId
    )
  );
}

export const failureCount = derived(watchState, ($watches) =>
  $watches.filter((w) => {
    if (!w.lastResult) return false;
    return w.lastResult.outcome === "failure";
  }).length
);
