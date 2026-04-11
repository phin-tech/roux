import { writeToSession } from "$lib/tauri";
import { log, logError } from "$lib/logging";
import type { SpawnProfile } from "./profiles";

/**
 * Type a profile's setup and startup commands into an existing shell PTY.
 *
 * `setupCommand` (if any) goes in first and always ends with a newline so
 * it executes before `startupCommand`. `startupCommand` (if any) is then
 * typed, and whether it auto-runs is controlled by the profile's
 * `startupBehavior`:
 *
 * - `"autoRun"` (default): append `\n` so the shell executes immediately.
 * - `"typeOnly"`: leave the command at the shell prompt so the user can
 *   review / edit before pressing Enter.
 *
 * `"typeOnly"` only affects `startupCommand` — setup always runs, because
 * the user explicitly opted into the profile and is not going to inspect
 * setup output mid-stream.
 *
 * No-op for profiles that have neither command (e.g. the built-in
 * `Plain shell` profile). Safe to call on a just-spawned PTY; the
 * backend's pending-output channel buffers writes until the reader is
 * attached.
 */
export async function runProfileInPane(
  ptyId: string,
  profile: SpawnProfile,
): Promise<void> {
  if (!profile.setupCommand && !profile.startupCommand) return;

  try {
    if (profile.setupCommand && profile.setupCommand.trim().length > 0) {
      log(`runProfileInPane(${ptyId}): typing setup command for profile "${profile.id}"`);
      await writeToSession(ptyId, profile.setupCommand);
      await writeToSession(ptyId, "\n");
    }

    if (profile.startupCommand && profile.startupCommand.trim().length > 0) {
      const suffix = (profile.startupBehavior ?? "autoRun") === "typeOnly" ? "" : "\n";
      log(
        `runProfileInPane(${ptyId}): typing startup command for profile "${profile.id}" (behavior=${profile.startupBehavior ?? "autoRun"})`,
      );
      await writeToSession(ptyId, profile.startupCommand);
      if (suffix) await writeToSession(ptyId, suffix);
    }
  } catch (e) {
    logError(`runProfileInPane(${ptyId}) failed`, e);
  }
}
