import { checkSmolvmBinary, writeToSession } from "$lib/tauri";
import { log, logError } from "$lib/logging";
import type { SpawnProfile } from "./profiles";
import { appendAgentSystemPrompt } from "./agentPrompt";
import {
  extractBinaryFromStartupCommand,
  formatGuestAgentMissingComment,
  knownAgentInstallCommand,
} from "./agentBinary";

/**
 * Valid POSIX-ish shell identifier: must start with a letter or underscore
 * and contain only word characters thereafter. Env entries whose keys don't
 * match are silently dropped — they'd produce a broken `export` line and
 * their use is almost always a typo in the profile editor.
 */
const VALID_ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

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
  /** Smol-machine binding for the session this pane belongs to. When
   *  set and the profile's startup command targets a known agent
   *  (claude, codex), the runner preflights the agent's presence
   *  inside the guest before typing the command. If the agent is
   *  missing, a comment-block pointing at the panel's "Install" action
   *  is typed instead — saves the user from a confusing
   *  `/bin/sh: claude: not found`. */
  smolMachineName?: string | null;
}

/**
 * Type a profile's environment, working-directory override, and setup /
 * startup commands into an existing shell PTY.
 *
 * Order (each line written as a separate chunk so ordering is explicit and
 * the PTY receives human-looking input):
 *
 *   1. `cd 'escaped/cwdOverride'` (if set and non-empty)
 *   2. `export KEY='escaped value'` for each valid env entry
 *   3. `setupCommand` (always auto-run — the user opted into the profile
 *      and is not going to inspect setup mid-stream)
 *   4. `startupCommand`, with trailing `\n` unless `startupBehavior ==
 *      "typeOnly"`, in which case the command sits at the prompt for
 *      review before the user presses Enter.
 *
 * No-op for profiles that have none of cwdOverride / env / setupCommand /
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
  const envEntries = Object.entries(profile.env ?? {}).filter(
    ([name]) => VALID_ENV_NAME.test(name),
  );
  const hasSetup = !!profile.setupCommand && profile.setupCommand.trim().length > 0;
  const baseStartup = profile.startupCommand ?? "";
  const startupCommand = appendAgentSystemPrompt(
    baseStartup,
    profile.provider,
    opts.appendSystemPrompt ?? "",
  );
  const hasStartup = startupCommand.trim().length > 0;

  if (!cwdOverride && envEntries.length === 0 && !hasSetup && !hasStartup) {
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

  for (const [name, value] of envEntries) {
    await writeToSession(ptyId, `export ${name}=${shellSingleQuote(value)}`);
    await writeToSession(ptyId, "\n");
  }

  if (hasSetup) {
    log(`runProfileInPane(${ptyId}): typing setup command for profile "${profile.id}"`);
    await writeToSession(ptyId, profile.setupCommand!);
    await writeToSession(ptyId, "\n");
  }

  if (hasStartup) {
    // Smol-VM-bound preflight: when the profile targets a known agent
    // (claude / codex) and the guest doesn't have it on PATH, type a
    // comment block pointing at the panel's Install action instead
    // of the bad startup command. The shell stays alive at a prompt;
    // the user reads the comment, installs, and re-runs.
    const machineName = opts.smolMachineName?.trim() ?? "";
    if (machineName) {
      const binary = extractBinaryFromStartupCommand(startupCommand);
      const installCmd = binary ? knownAgentInstallCommand(binary) : null;
      if (binary && installCmd) {
        try {
          const found = await checkSmolvmBinary(machineName, binary);
          if (!found) {
            log(
              `runProfileInPane(${ptyId}): ${binary} missing in smol machine '${machineName}'; substituting install hint`,
            );
            const block = formatGuestAgentMissingComment(
              binary,
              machineName,
              installCmd,
            );
            await writeToSession(ptyId, block);
            await writeToSession(ptyId, "\n");
            return;
          }
        } catch (err) {
          // Preflight is best-effort. If the smolvm probe fails (e.g.
          // machine just stopped between `ensure_machine_running` and
          // here), fall through to the normal type-the-command path —
          // the user will get the raw `command not found` error and
          // can investigate. Logging keeps the failure traceable.
          logError(
            `runProfileInPane(${ptyId}): smol-binary preflight failed for ${binary} in '${machineName}': ${err}`,
          );
        }
      }
    }

    const suffix = (profile.startupBehavior ?? "autoRun") === "typeOnly" ? "" : "\n";
    log(
      `runProfileInPane(${ptyId}): typing startup command for profile "${profile.id}" (behavior=${profile.startupBehavior ?? "autoRun"})`,
    );
    await writeToSession(ptyId, startupCommand);
    if (suffix) await writeToSession(ptyId, suffix);
  }
}
