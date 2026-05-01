/**
 * Provider-aware "extend the system prompt" injection.
 *
 * Each first-class agent CLI exposes a different hook for biasing the
 * conversation without hijacking the user's first turn:
 *
 *   - Claude: `--append-system-prompt '<text>'`
 *   - Codex:  `-c instructions='<text>'`
 *
 * Anything else (plain shells, third-party agents) leaves the startup
 * command unchanged. Capability detection lives on `agentPromptFlag`,
 * which returns `null` for unsupported providers — callers that care can
 * still surface the value via the `ROUX_PROJECT_PROMPT` env var and
 * splice it manually from a custom profile's `startupCommand`.
 *
 * Usage shapes (callers pick whichever composes best):
 *
 *   // 1. raw string transform — when you already have a command line
 *   const cmd = appendAgentSystemPrompt(profile.startupCommand, profile.provider, prompt);
 *
 *   // 2. profile transform — when you want a profile to carry the change
 *   const wrapped = withAppendedSystemPrompt(profile, prompt);
 *
 * Designed to compose: applying a layout-level prompt and then a
 * project-level prompt is just two `withAppendedSystemPrompt` calls.
 */

import type { SpawnProfile } from "./profiles";

type Provider = SpawnProfile["provider"] | undefined;

/**
 * Wrap a value in single quotes for safe inclusion in a shell command.
 * Single-quoted strings suppress every form of shell expansion — $, `,
 * \, glob chars, all of it — except for the single quote itself, which
 * we splice in with the standard `'\''` dance.
 */
function shellSingleQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/**
 * The flag prefix this provider uses for "append to the system prompt".
 * Returns `null` for providers that don't have a documented hook so callers
 * can fall back to env-var or no-op behavior without guessing.
 *
 * Exposed separately from `appendAgentSystemPrompt` for callers that need
 * to render UI hints (e.g. "this profile is Claude — prompt will be
 * passed via `--append-system-prompt`").
 */
export function agentPromptFlag(provider: Provider): string | null {
  if (provider === "claude") return "--append-system-prompt";
  if (provider === "codex") return "-c instructions=";
  return null;
}

/**
 * Splice `prompt` into `startupCommand` using the provider-appropriate
 * flag. The prompt is shell-quoted so it survives arbitrary content
 * (newlines, quotes, `$`, backticks, etc.).
 *
 * No-ops when the prompt is empty/whitespace, when the provider has no
 * known flag, or when the startupCommand itself is empty (a profile with
 * no command has nothing for the agent to read).
 *
 * Codex's flag has no separating space (`-c instructions=foo`); Claude's
 * does (`--append-system-prompt foo`). The function handles both.
 */
export function appendAgentSystemPrompt(
  startupCommand: string,
  provider: Provider,
  prompt: string,
): string {
  const trimmed = prompt.trim();
  if (!trimmed) return startupCommand;
  if (!startupCommand.trim()) return startupCommand;
  const flag = agentPromptFlag(provider);
  if (!flag) return startupCommand;
  const quoted = shellSingleQuote(trimmed);
  // Claude: "--append-system-prompt 'X'" (space before value)
  // Codex:  "-c instructions='X'"        (no space; key=value form)
  const sep = flag.endsWith("=") ? "" : " ";
  return `${startupCommand} ${flag}${sep}${quoted}`;
}

/**
 * Return a new profile with `prompt` appended to its `startupCommand`.
 * The original profile is not mutated. When the prompt has no effect
 * (empty / unsupported provider / no startupCommand), the profile is
 * returned unchanged so callers can chain calls without churn.
 */
export function withAppendedSystemPrompt(
  profile: SpawnProfile,
  prompt: string,
): SpawnProfile {
  const trimmed = prompt.trim();
  if (!trimmed) return profile;
  const base = profile.startupCommand ?? "";
  const next = appendAgentSystemPrompt(base, profile.provider, trimmed);
  if (next === base) return profile;
  return { ...profile, startupCommand: next };
}
