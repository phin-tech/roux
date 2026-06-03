import { writeToSession } from "$lib/tauri";
import { log } from "$lib/logging";
import type { SpawnProfile } from "./profiles";
import { appendAgentSystemPrompt } from "./agentPrompt";

/**
 * Wrap a value in single quotes for safe inclusion in a shell command.
 * Single-quoted strings suppress every form of shell expansion — $, `,
 * \, glob chars, all of it — except for the single quote itself, which
 * we splice in with the standard `'\''` dance.
 *
 * This is how we defend against command injection from a malicious or
 * sloppily written profile: a user profile with
 * `env: { MSG: "'; rm -rf / #" }` otherwise becomes
 * `export MSG=''; rm -rf / #'` at the shell, which is game over.
 */
function shellSingleQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export interface RunProfileOptions {
  /** Free-form text to splice into the agent CLI's startup command via
   *  the provider-appropriate flag — see `appendAgentSystemPrompt`. The
   *  caller decides where this comes from (project, layout, ad-hoc),
   *  so this option stays provider/source-agnostic. */
  appendSystemPrompt?: string;
}

/**
 * Type a profile's working-directory override and setup / startup commands
 * into an existing shell PTY. Profile environment and preflight commands are
 * resolved by the daemon/runtime before the shell process starts.
 *
 * Order (each line written as a separate chunk so ordering is explicit and
 * the PTY receives human-looking input):
 *
 *   1. `cd 'escaped/cwdOverride'` (if set and non-empty)
 *   2. `setupCommand` (always auto-run — the user opted into the profile
 *      and is not going to inspect setup mid-stream)
 *   3. `startupCommand`, with trailing `\n` unless `startupBehavior ==
 *      "typeOnly"`, in which case the command sits at the prompt for
 *      review before the user presses Enter.
 *
 * No-op for profiles that have none of cwdOverride / setupCommand /
 * startupCommand (e.g. the built-in `Plain shell` profile). Safe to call
 * on a just-spawned PTY; the backend's pending-output channel buffers
 * writes until the reader is attached.
 */
export async function runProfileInPane(
  ptyId: string,
  profile: SpawnProfile,
  opts: RunProfileOptions = {},
): Promise<void> {
  const cwdOverride = profile.cwdOverride?.trim() ?? "";
  const hasSetup =
    !!profile.setupCommand && profile.setupCommand.trim().length > 0;
  const baseStartup = profile.startupCommand ?? "";
  const startupCommand = appendAgentSystemPrompt(
    baseStartup,
    profile.provider,
    opts.appendSystemPrompt ?? "",
  );
  const hasStartup = startupCommand.trim().length > 0;

  if (!cwdOverride && !hasSetup && !hasStartup) {
    return;
  }

  // Errors propagate. The runner used to log-and-swallow, which meant a
  // profile with a busted setupCommand silently produced a dead-looking
  // pane and the user had no way to find out. Callers now get the error
  // and decide how to surface it (inline in the new-session dialog, as a
  // notification from the re-run button, etc.). A wrapped exception
  // includes the profile id so callers can quote it in the UI without
  // re-parsing the underlying IO error.
  if (cwdOverride) {
    log(
      `runProfileInPane(${ptyId}): cd to override for profile "${profile.id}"`,
    );
    await writeToSession(ptyId, `cd ${shellSingleQuote(cwdOverride)}`);
    await writeToSession(ptyId, "\n");
  }

  if (hasSetup) {
    log(
      `runProfileInPane(${ptyId}): typing setup command for profile "${profile.id}"`,
    );
    await writeToSession(ptyId, profile.setupCommand!);
    await writeToSession(ptyId, "\n");
  }

  if (hasStartup) {
    const suffix =
      (profile.startupBehavior ?? "autoRun") === "typeOnly" ? "" : "\n";
    log(
      `runProfileInPane(${ptyId}): typing startup command for profile "${profile.id}" (behavior=${profile.startupBehavior ?? "autoRun"})`,
    );
    await writeToSession(ptyId, startupCommand);
    if (suffix) await writeToSession(ptyId, suffix);
  }
}
