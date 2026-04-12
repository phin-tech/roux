import { writable, get, derived, type Readable } from "svelte/store";
import { commands } from "$lib/bindings";
import type {
  Provider,
  ProfileSource,
  SpawnProfile,
  StartupBehavior,
} from "$lib/bindings";
import { log } from "$lib/logging";

export type { Provider, ProfileSource, SpawnProfile, StartupBehavior };

/**
 * A persisted reference to a spawn profile attached to a pane at creation
 * time. Two shapes exist so that panes survive restart even when the user
 * deletes or renames the profile they were launched from:
 *
 * - `registered`: stable id pointer. On restore the id is re-resolved from
 *   the current registry; if it's gone, the pane still comes back as a live
 *   shell, minus the "Re-run profile" button.
 * - `inline`: the entire profile is captured on the pane. Ad-hoc "Custom…"
 *   panes use this so they don't depend on settings state at restore time.
 */
export type SpawnProfileRef =
  | { kind: "registered"; id: string }
  | { kind: "inline"; profile: SpawnProfile };

/**
 * The built-in segment of the registry, populated once at frontend startup
 * by `loadBuiltinProfiles`. Stays stable across setting edits — settings
 * only drive the user segment.
 */
const builtinProfiles = writable<SpawnProfile[]>([]);

/**
 * The user segment of the registry, sourced from
 * `RouxSettings.spawnProfiles`. Updated by `setUserProfiles` whenever the
 * settings store emits a change.
 */
const userProfiles = writable<SpawnProfile[]>([]);

/**
 * Flat registry keyed by profile id. Built-in profiles come first so they
 * always appear in the picker even if a user profile shadows them; the user
 * entry wins on id collision because later writes override.
 *
 * `kind: "inline"` refs are NOT in the registry — they carry their profile
 * inline on each pane and are looked up via the ref itself.
 */
export const profileRegistry: Readable<Map<string, SpawnProfile>> = derived(
  [builtinProfiles, userProfiles],
  ([$builtin, $user]) => {
    const map = new Map<string, SpawnProfile>();
    for (const p of $builtin) map.set(p.id, p);
    for (const p of $user) map.set(p.id, { ...p, source: "user" });
    return map;
  },
);

/** Ordered list view over the registry, for picker menus. */
export const profileList: Readable<SpawnProfile[]> = derived(
  profileRegistry,
  ($registry) => Array.from($registry.values()),
);

/**
 * Load the built-in profile segment from the backend. Called once at app
 * start; safe to call again if provider modules or settings change in ways
 * that alter the derived `startupCommand`.
 */
export async function loadBuiltinProfiles(): Promise<void> {
  try {
    const profiles = await commands.getBuiltinProfiles();
    builtinProfiles.set(profiles);
    log(`loadBuiltinProfiles: loaded ${profiles.length} built-in profile(s)`);
  } catch (e) {
    log(`loadBuiltinProfiles: failed to load — ${e}`);
    builtinProfiles.set([]);
  }
}

/**
 * Replace the user profile segment. Called whenever the settings store
 * emits a change. The settings loader already stamps `source: "user"` on
 * every entry on the Rust side; we re-stamp here so the frontend never
 * trusts a field that could be forged.
 */
export function setUserProfiles(profiles: SpawnProfile[] | undefined): void {
  userProfiles.set(
    (profiles ?? []).map((p) => ({ ...p, source: "user" as ProfileSource })),
  );
}

/**
 * Resolve a profile ref to its concrete `SpawnProfile`, or `null` when a
 * registered profile has been deleted out from under a restored pane.
 */
export function resolveProfileRef(
  ref: SpawnProfileRef | undefined,
): SpawnProfile | null {
  if (!ref) return null;
  if (ref.kind === "inline") return ref.profile;
  return get(profileRegistry).get(ref.id) ?? null;
}

/**
 * Test-only reset hook. Clears both segments of the registry so each test
 * starts from a known-empty state.
 */
export function resetProfileRegistry(): void {
  builtinProfiles.set([]);
  userProfiles.set([]);
}
