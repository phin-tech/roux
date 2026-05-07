// Helpers for the smol-VM-bound profile-replay preflight.
//
// When a session is bound to a smol machine and the user spawns a
// profile whose startup command targets a known agent (claude, codex),
// we want to detect "agent isn't installed in the guest" *before*
// typing the command into the shell — otherwise the user sees a raw
// `/bin/sh: claude: not found`. Instead we type a comment block
// pointing at the panel's "Install Claude" action.
//
// This module is purely synchronous and stateless. The actual `which`
// shellout to smolvm and the comment-vs-command branching live in
// `profileRunner.ts`.

/**
 * Extract the leading binary name from a profile's `startupCommand`.
 * Handles the common shapes Roux's providers emit:
 *
 *   "claude"                              → "claude"
 *   "claude --model opus"                 → "claude"
 *   "/usr/local/bin/claude"               → "claude"
 *   "'/Applications/Claude Code.app/claude'" → "claude"
 *   '"claude --foo"'                      → "claude"
 *   "FOO=bar BAZ=qux claude --model x"    → "claude"  (skip env prefix)
 *   "" / "   "                            → null
 *
 * NOT a general shell parser. Stops at the first non-`KEY=VALUE` token
 * and treats it as the binary, then takes the basename. Returns null
 * when no binary is identifiable. The basename step is deliberate:
 * profile-replay types absolute paths (e.g. `/Applications/Claude
 * Code.app/claude`) on macOS hosts but those paths don't exist in a
 * Linux guest — only the bare command name is meaningful inside the
 * VM, and that's what we want to preflight.
 */
export function extractBinaryFromStartupCommand(cmd: string): string | null {
  let i = 0;
  const len = cmd.length;

  // Walk the input one token at a time, skipping leading KEY=VAL env
  // prefixes. The first non-env token is the binary. A token is
  // either a quoted span (starting with `'` or `"`) terminated by
  // the matching quote, or a whitespace-terminated bare run.
  while (i < len) {
    // Skip whitespace.
    while (i < len && /\s/.test(cmd[i])) i++;
    if (i >= len) break;

    const startedAt = i;
    let token: string;

    if (cmd[i] === "'" || cmd[i] === '"') {
      // Quoted token: find the matching close quote. If the closer is
      // missing (unterminated), take everything to end-of-string and
      // strip the leading quote. The basename step still produces
      // sensible output for the common `'/path with space/bin'` case.
      const quote = cmd[i];
      const close = cmd.indexOf(quote, i + 1);
      if (close === -1) {
        token = cmd.slice(i + 1);
        i = len;
      } else {
        token = cmd.slice(i + 1, close);
        i = close + 1;
      }
    } else {
      // Bare token: read until next whitespace.
      let end = i;
      while (end < len && !/\s/.test(cmd[end])) end++;
      token = cmd.slice(i, end);
      i = end;
    }

    // KEY=VAL env prefix (only meaningful for *bare* tokens — a
    // quoted span isn't a shell env assignment).
    const isEnvPrefix =
      cmd[startedAt] !== "'" &&
      cmd[startedAt] !== '"' &&
      /^[A-Za-z_][A-Za-z0-9_]*=/.test(token);
    if (isEnvPrefix) continue;

    // basename — last `/`-separated segment. Empty after basename
    // (e.g. token ended in `/`) means no identifiable binary.
    const lastSlash = token.lastIndexOf("/");
    const basename = lastSlash >= 0 ? token.slice(lastSlash + 1) : token;
    return basename.length > 0 ? basename : null;
  }

  return null;
}

/**
 * Format the "agent not installed in guest" comment block we type into
 * the shell instead of the user's startup command. Each line is
 * `# `-prefixed so accidentally pasting the block back into a shell
 * runs nothing dangerous. Returns the body without a trailing newline;
 * the caller appends one.
 */
export function formatGuestAgentMissingComment(
  agent: string,
  machineName: string,
  installCommand: string,
): string {
  return [
    `# ${agent} is not installed in smol machine '${machineName}'.`,
    `#`,
    `# Install it with one click via Smol Machines panel → Install ${capitalize(agent)},`,
    `# or run yourself:`,
    `#   ${installCommand}`,
  ].join("\n");
}

function capitalize(s: string): string {
  return s.length === 0 ? s : s[0].toUpperCase() + s.slice(1);
}

/**
 * Map a known-agent binary name to its install command. Mirrors
 * `roux_smolvm::KnownAgent::install_command` on the Rust side — keep
 * these two in sync. Claude uses the official self-contained
 * installer; Codex is still npm-distributed (the guest needs node +
 * npm on PATH for Codex). Returning `null` for unknown agents lets
 * the preflight skip them.
 */
export function knownAgentInstallCommand(binary: string): string | null {
  switch (binary.toLowerCase()) {
    case "claude":
      return "curl -fsSL https://claude.ai/install.sh | bash";
    case "codex":
      return "npm install -g @openai/codex";
    default:
      return null;
  }
}
