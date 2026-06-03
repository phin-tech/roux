import { writable, get, derived, type Readable } from "svelte/store";
import { commands } from "$lib/bindings";
import type { LayoutSpec, LayoutSource } from "$lib/bindings";
import { log } from "$lib/logging";

export type { LayoutSpec, LayoutSource };

/**
 * The built-in segment of the layout registry, populated once at frontend
 * startup by `loadBuiltinLayouts`.
 */
const builtinLayouts = writable<LayoutSpec[]>([]);

/**
 * The user segment of the layout registry, sourced from
 * `~/.config/roux/layouts/*.kdl`. Updated by `loadUserLayouts`.
 */
const userLayouts = writable<LayoutSpec[]>([]);

/**
 * Flat registry keyed by layout id. Built-in layouts come first; the user
 * entry wins on id collision because later writes override.
 */
export const layoutRegistry: Readable<Map<string, LayoutSpec>> = derived(
  [builtinLayouts, userLayouts],
  ([$builtin, $user]) => {
    const map = new Map<string, LayoutSpec>();
    for (const l of $builtin) map.set(l.id, l);
    for (const l of $user)
      map.set(l.id, { ...l, source: "user" as LayoutSource });
    return map;
  },
);

/** Ordered list view over the registry, for picker menus. */
export const layoutList: Readable<LayoutSpec[]> = derived(
  layoutRegistry,
  ($registry) => Array.from($registry.values()),
);

/**
 * Load the built-in layout segment from the backend. Called once at app
 * start; safe to call again.
 */
export async function loadBuiltinLayouts(): Promise<void> {
  try {
    const layouts = await commands.getBuiltinLayouts();
    builtinLayouts.set(layouts);
    log(`loadBuiltinLayouts: loaded ${layouts.length} built-in layout(s)`);
  } catch (e) {
    log(`loadBuiltinLayouts: failed to load — ${e}`);
    builtinLayouts.set([]);
  }
}

/**
 * Load user-authored layouts from the backend. Called once at app start;
 * safe to call again.
 */
export async function loadUserLayouts(): Promise<void> {
  try {
    const layouts = await commands.getUserLayouts();
    userLayouts.set(layouts);
    log(`loadUserLayouts: loaded ${layouts.length} user layout(s)`);
  } catch (e) {
    log(`loadUserLayouts: failed to load — ${e}`);
    userLayouts.set([]);
  }
}

/**
 * Resolve a layout by id from the merged registry, or null if not found.
 */
export function resolveLayoutById(id: string): LayoutSpec | null {
  return get(layoutRegistry).get(id) ?? null;
}

/**
 * Test-only reset hook. Clears both segments of the registry so each test
 * starts from a known-empty state.
 */
export function resetLayoutRegistry(): void {
  builtinLayouts.set([]);
  userLayouts.set([]);
}
