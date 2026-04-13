/**
 * Wait for a freshly spawned shell to be ready to receive keystrokes
 * before typing setup / startup commands into it.
 *
 * The classic race: roux spawns a shell, xterm attaches, and we start
 * writing `claude\n` immediately. With a slow rc file, zsh-autosuggestions,
 * starship, etc., zsh has not yet initialized ZLE (its line editor) and
 * silently discards input that arrives before it does. The user sees a
 * shell prompt in the new worktree pane and no `claude` running.
 *
 * Resolution strategy, in order of preference:
 *
 *   1. OSC 133;A — the standard "prompt start" marker emitted by modern
 *      shell prompt frameworks (starship, p10k, zsh's own vcs_info scripts,
 *      bash-preexec, etc.). If we see it, the prompt is up and ZLE is
 *      ready. This is the only authoritative signal.
 *
 *   2. Output quiescence — once we've seen any bytes at all, wait for a
 *      brief silence (default 200ms) and assume the shell has settled.
 *      Not perfect (a prompt with a live clock keeps emitting), but
 *      correct for the vast majority of prompts.
 *
 *   3. Hard timeout — at `timeoutMs` (default 5s) we give up and let
 *      writes fire blind. Logged as a warning so it's diagnosable.
 *
 * The detector subscribes to `ptyOutputBus`, which replays recent bytes
 * for this ptyId, so we don't miss the initial output flush that may have
 * arrived before this function was called.
 */

import { onPtyOutput } from "./ptyOutputBus";
import { log } from "$lib/logging";

// ESC ] 1 3 3 ; A
const OSC_133_A = new Uint8Array([0x1b, 0x5d, 0x31, 0x33, 0x33, 0x3b, 0x41]);

function containsSubarray(hay: Uint8Array, needle: Uint8Array): boolean {
  if (needle.length === 0 || hay.length < needle.length) return false;
  outer: for (let i = 0; i <= hay.length - needle.length; i++) {
    for (let j = 0; j < needle.length; j++) {
      if (hay[i + j] !== needle[j]) continue outer;
    }
    return true;
  }
  return false;
}

export type ShellReadyReason = "osc133" | "quiet" | "timeout";

export interface ShellReadyOptions {
  /** Silence window after first byte before we declare "ready". */
  quietMs?: number;
  /** Hard ceiling; after this we warn and resolve anyway. */
  timeoutMs?: number;
}

export function waitForShellReady(
  ptyId: string,
  opts: ShellReadyOptions = {},
): Promise<ShellReadyReason> {
  const quietMs = opts.quietMs ?? 200;
  const timeoutMs = opts.timeoutMs ?? 5000;

  return new Promise((resolve) => {
    let resolved = false;
    let quietTimer: ReturnType<typeof setTimeout> | null = null;
    let hardTimeout: ReturnType<typeof setTimeout> | null = null;
    // Must be a mutable holder rather than the `const` returned by
    // `onPtyOutput`: that subscribe call invokes the listener synchronously
    // with replayed bytes, so the listener runs before the subscribe call
    // returns. A `const unsubscribe = onPtyOutput(...)` would be in the
    // temporal dead zone during that synchronous replay and finish() would
    // throw.
    let unsubscribe: (() => void) | null = null;

    const finish = (reason: ShellReadyReason) => {
      if (resolved) return;
      resolved = true;
      if (quietTimer !== null) clearTimeout(quietTimer);
      if (hardTimeout !== null) clearTimeout(hardTimeout);
      if (unsubscribe) unsubscribe();
      if (reason === "timeout") {
        log(
          `waitForShellReady(${ptyId}): no prompt / quiet period within ${timeoutMs}ms, proceeding anyway`,
        );
      }
      resolve(reason);
    };

    unsubscribe = onPtyOutput(ptyId, (bytes) => {
      if (resolved) return;
      if (containsSubarray(bytes, OSC_133_A)) {
        finish("osc133");
        return;
      }
      if (quietTimer !== null) clearTimeout(quietTimer);
      quietTimer = setTimeout(() => finish("quiet"), quietMs);
    });

    if (!resolved) {
      hardTimeout = setTimeout(() => finish("timeout"), timeoutMs);
    }
  });
}

export type WaitForOutputResult =
  | { kind: "matched"; text: string }
  | { kind: "timeout" };

export interface WaitForOutputOptions {
  /** Give up and resolve `{ kind: "timeout" }` after this many ms. */
  timeoutMs?: number;
  /**
   * Max bytes of decoded output to retain while scanning. Longer matchers
   * that straddle chunk boundaries need a bigger window; default is
   * comfortable for typical CLI output.
   */
  windowBytes?: number;
}

/**
 * Watch a pane's output stream until `matcher` is found in the decoded
 * text, or `timeoutMs` elapses. The matcher can be a plain substring or
 * a RegExp; it is applied to a rolling UTF-8-decoded window so matches
 * that straddle chunk boundaries still fire.
 *
 * Useful for profile flows whose next step depends on a previous command
 * finishing — e.g. wait for `Successfully logged into` after `aws sso
 * login` before typing the command the user actually cares about.
 *
 * Notes:
 * - Matching is done on decoded text, not raw bytes, so ANSI escape
 *   sequences in the stream can interrupt literal substring matches.
 *   For CLI success messages this is rarely a problem; for patterns
 *   that might be ANSI-colored, prefer a RegExp that tolerates escapes
 *   or match on a known-stable substring.
 * - The replay buffer means a match present before this call returns may
 *   resolve synchronously-ish on the first subscription tick.
 */
export function waitForOutput(
  ptyId: string,
  matcher: string | RegExp,
  opts: WaitForOutputOptions = {},
): Promise<WaitForOutputResult> {
  const timeoutMs = opts.timeoutMs ?? 30_000;
  const windowBytes = opts.windowBytes ?? 64 * 1024;

  return new Promise((resolve) => {
    let resolved = false;
    let hardTimeout: ReturnType<typeof setTimeout> | null = null;
    // See the note in waitForShellReady: replayed bytes fire the listener
    // synchronously from inside the subscribe call, so we can't use a
    // `const` here.
    let unsubscribe: (() => void) | null = null;

    const decoder = new TextDecoder("utf-8", { fatal: false });
    let window = "";

    const finish = (result: WaitForOutputResult) => {
      if (resolved) return;
      resolved = true;
      if (hardTimeout !== null) clearTimeout(hardTimeout);
      if (unsubscribe) unsubscribe();
      resolve(result);
    };

    const tryMatch = (): string | null => {
      if (typeof matcher === "string") {
        return window.includes(matcher) ? matcher : null;
      }
      const m = window.match(matcher);
      return m ? m[0] : null;
    };

    unsubscribe = onPtyOutput(ptyId, (bytes) => {
      if (resolved) return;
      window += decoder.decode(bytes, { stream: true });
      if (window.length > windowBytes) {
        window = window.slice(window.length - windowBytes);
      }
      const hit = tryMatch();
      if (hit !== null) finish({ kind: "matched", text: hit });
    });

    if (!resolved) {
      hardTimeout = setTimeout(() => finish({ kind: "timeout" }), timeoutMs);
    }
  });
}
