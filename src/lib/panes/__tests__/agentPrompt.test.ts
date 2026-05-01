import { describe, expect, it } from "vitest";

import {
  agentPromptFlag,
  appendAgentSystemPrompt,
  withAppendedSystemPrompt,
} from "../agentPrompt";
import type { SpawnProfile } from "../profiles";

function profile(overrides: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id: "test",
    name: "Test",
    source: "builtin",
    ...overrides,
  };
}

describe("agentPromptFlag", () => {
  it("maps Claude to --append-system-prompt", () => {
    expect(agentPromptFlag("claude")).toBe("--append-system-prompt");
  });

  it("maps Codex to -c instructions= (no separating space)", () => {
    expect(agentPromptFlag("codex")).toBe("-c instructions=");
  });

  it("returns null for unknown / undefined providers", () => {
    expect(agentPromptFlag(undefined)).toBeNull();
    expect(agentPromptFlag(null as unknown as undefined)).toBeNull();
  });
});

describe("appendAgentSystemPrompt", () => {
  it("appends the Claude flag with a space separator and quoted value", () => {
    expect(appendAgentSystemPrompt("claude", "claude", "be terse")).toBe(
      "claude --append-system-prompt 'be terse'",
    );
  });

  it("appends the Codex flag inline (key=value form)", () => {
    expect(appendAgentSystemPrompt("codex", "codex", "be terse")).toBe(
      "codex -c instructions='be terse'",
    );
  });

  it("escapes single quotes in the prompt", () => {
    expect(appendAgentSystemPrompt("claude", "claude", "it's fine")).toBe(
      "claude --append-system-prompt 'it'\\''s fine'",
    );
  });

  it("preserves shell metacharacters via single-quote wrapping", () => {
    // Single-quoting suppresses $, `, glob chars, etc. Critical for
    // defending against an injected `$(rm -rf /)` in a project prompt.
    expect(appendAgentSystemPrompt("claude", "claude", "$(whoami)")).toBe(
      "claude --append-system-prompt '$(whoami)'",
    );
  });

  it("returns the original command when prompt is empty / whitespace", () => {
    expect(appendAgentSystemPrompt("claude", "claude", "")).toBe("claude");
    expect(appendAgentSystemPrompt("claude", "claude", "   \n  ")).toBe(
      "claude",
    );
  });

  it("returns the original command for providers without a known flag", () => {
    // Plain shell, third-party agents, etc. Caller still has the env-var
    // path (`ROUX_PROJECT_PROMPT`) for these.
    expect(appendAgentSystemPrompt("foo", undefined, "bias")).toBe("foo");
  });

  it("returns the original (empty) command when there is nothing to append to", () => {
    // A profile with no startupCommand has nothing for the agent CLI to
    // read — appending a flag without a binary in front would type a
    // bare `--append-system-prompt 'X'` line into the shell.
    expect(appendAgentSystemPrompt("", "claude", "bias")).toBe("");
    expect(appendAgentSystemPrompt("   ", "claude", "bias")).toBe("   ");
  });

  it("composes — applying twice produces two appended flags", () => {
    // Layouts could layer their own prompt on top of a project prompt.
    const once = appendAgentSystemPrompt("claude", "claude", "layer-a");
    const twice = appendAgentSystemPrompt(once, "claude", "layer-b");
    expect(twice).toBe(
      "claude --append-system-prompt 'layer-a' --append-system-prompt 'layer-b'",
    );
  });
});

describe("withAppendedSystemPrompt", () => {
  it("returns a new profile with the modified startupCommand", () => {
    const p = profile({ provider: "claude", startupCommand: "claude" });
    const wrapped = withAppendedSystemPrompt(p, "be terse");
    expect(wrapped).not.toBe(p);
    expect(wrapped.startupCommand).toBe(
      "claude --append-system-prompt 'be terse'",
    );
    // Original is not mutated.
    expect(p.startupCommand).toBe("claude");
  });

  it("returns the same reference when the prompt is a no-op", () => {
    const p = profile({ provider: "claude", startupCommand: "claude" });
    expect(withAppendedSystemPrompt(p, "")).toBe(p);
    expect(withAppendedSystemPrompt(p, "   ")).toBe(p);
  });

  it("returns the same reference for providers with no known flag", () => {
    const p = profile({ startupCommand: "bun run dev" });
    expect(withAppendedSystemPrompt(p, "bias")).toBe(p);
  });

  it("composes — chained calls layer multiple prompts", () => {
    const p = profile({ provider: "codex", startupCommand: "codex" });
    const layered = withAppendedSystemPrompt(
      withAppendedSystemPrompt(p, "layout-prompt"),
      "project-prompt",
    );
    expect(layered.startupCommand).toBe(
      "codex -c instructions='layout-prompt' -c instructions='project-prompt'",
    );
  });
});
