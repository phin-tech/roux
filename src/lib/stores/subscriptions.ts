import { writable, get } from "svelte/store";
import {
  onBusSubscriptionEvent,
  subscriptionsCreate as tauriCreate,
  subscriptionsDelete as tauriDelete,
  subscriptionsList,
} from "$lib/tauri";
import type { BusSubscription, BusSubscriptionEvent } from "$lib/tauri";

/**
 * Authoritative store of all bus subscriptions, latest-first by
 * createdAt. Hydrated once on app start and kept in sync via the
 * `subscription-event` Tauri channel.
 */
export const subscriptions = writable<BusSubscription[]>([]);

/** Hydrate the store from the backend. Call once on app start. */
export async function hydrateSubscriptions(): Promise<void> {
  try {
    const subs = await subscriptionsList({});
    subs.sort((a, b) => b.createdAt - a.createdAt);
    subscriptions.set(subs);
  } catch (err) {
    console.error("Failed to hydrate bus subscriptions", err);
  }
}

/**
 * Apply a `subscription-event` payload to the store. Pure function so
 * tests can exercise it without a Tauri runtime.
 */
export function applySubscriptionEvent(event: BusSubscriptionEvent): void {
  subscriptions.update((current) => {
    if (event.kind === "created") {
      // Idempotent: replace any existing row with the same id, otherwise
      // prepend (newest-first).
      const without = current.filter((s) => s.id !== event.subscription.id);
      return [event.subscription, ...without];
    }
    return current.filter((s) => s.id !== event.id);
  });
}

let unlisten: (() => void) | null = null;

/** Wire the global event listener. Idempotent across hot reloads. */
export async function startSubscriptionEventListener(): Promise<void> {
  if (unlisten) return;
  unlisten = await onBusSubscriptionEvent(applySubscriptionEvent);
}

export function stopSubscriptionEventListener(): void {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
}

/** Create a new subscription. Throws on backend validation errors. */
export async function createSubscription(
  alias: string,
  pattern: string,
  projectId: string | null = null,
): Promise<BusSubscription> {
  return tauriCreate(alias, pattern, projectId);
}

/** Delete a subscription by id. Returns true when something was removed. */
export async function deleteSubscription(id: string): Promise<boolean> {
  return tauriDelete(id);
}

/**
 * Subscriptions for `alias`, sorted newest-first. Useful in templates
 * that need an alias-scoped view without re-fetching from the backend.
 */
export function subscriptionsForAlias(alias: string): BusSubscription[] {
  return get(subscriptions).filter((s) => s.alias === alias);
}
