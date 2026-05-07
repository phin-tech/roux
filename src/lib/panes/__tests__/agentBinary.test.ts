import { describe, expect, it } from "vitest";
import {
  extractBinaryFromStartupCommand,
  formatGuestAgentMissingComment,
  knownAgentInstallCommand,
} from "../agentBinary";

describe("extractBinaryFromStartupCommand", () => {
  it("returns the bare name", () => {
    expect(extractBinaryFromStartupCommand("claude")).toBe("claude");
  });

  it("ignores trailing flags", () => {
    expect(extractBinaryFromStartupCommand("claude --model opus")).toBe("claude");
  });

  it("takes basename of an absolute path", () => {
    expect(extractBinaryFromStartupCommand("/usr/local/bin/claude")).toBe(
      "claude",
    );
  });

  it("strips matching single quotes around a path with spaces", () => {
    expect(
      extractBinaryFromStartupCommand("'/Applications/Claude Code.app/claude'"),
    ).toBe("claude");
  });

  it("strips matching double quotes around the binary", () => {
    // `"claude" --foo` — quoted bare name + flag outside. The
    // realistic shape if someone wanted to pin the binary against
    // alias resolution.
    expect(extractBinaryFromStartupCommand('"claude" --foo')).toBe("claude");
  });

  it("handles a quoted absolute path with spaces in directory names", () => {
    expect(
      extractBinaryFromStartupCommand(
        "'/Users/me/My Apps/Claude Code.app/claude' --resume",
      ),
    ).toBe("claude");
  });

  it("skips leading KEY=VAL env prefixes", () => {
    expect(
      extractBinaryFromStartupCommand("FOO=bar BAZ=qux claude --model x"),
    ).toBe("claude");
  });

  it("returns null for empty / whitespace-only input", () => {
    expect(extractBinaryFromStartupCommand("")).toBeNull();
    expect(extractBinaryFromStartupCommand("   ")).toBeNull();
  });

  it("returns null when only env prefixes are present", () => {
    expect(extractBinaryFromStartupCommand("FOO=bar BAZ=qux")).toBeNull();
  });

  it("returns null for a path that ends in slash", () => {
    expect(extractBinaryFromStartupCommand("/usr/local/bin/")).toBeNull();
  });
});

describe("knownAgentInstallCommand", () => {
  it("uses the official curl installer for claude", () => {
    // Claude's self-contained installer works on minimal images
    // (Alpine, etc.) that don't ship npm. Codex still uses npm.
    expect(knownAgentInstallCommand("claude")).toBe(
      "curl -fsSL https://claude.ai/install.sh | bash",
    );
    expect(knownAgentInstallCommand("codex")).toBe(
      "npm install -g @openai/codex",
    );
  });

  it("is case-insensitive", () => {
    expect(knownAgentInstallCommand("Claude")).toBe(
      "curl -fsSL https://claude.ai/install.sh | bash",
    );
    expect(knownAgentInstallCommand("CODEX")).toBe(
      "npm install -g @openai/codex",
    );
  });

  it("returns null for unknown agents", () => {
    expect(knownAgentInstallCommand("aider")).toBeNull();
    expect(knownAgentInstallCommand("")).toBeNull();
  });
});

describe("formatGuestAgentMissingComment", () => {
  it("renders the multi-line block with all lines `# `-prefixed", () => {
    const out = formatGuestAgentMissingComment(
      "claude",
      "test-vm",
      "@anthropic-ai/claude",
    );
    for (const line of out.split("\n")) {
      expect(line.startsWith("#")).toBe(true);
    }
  });

  it("includes the machine name and install command verbatim", () => {
    const out = formatGuestAgentMissingComment(
      "codex",
      "alpine-1",
      "npm install -g @openai/codex",
    );
    expect(out).toContain("'alpine-1'");
    expect(out).toContain("npm install -g @openai/codex");
    expect(out).toContain("Install Codex");
  });

  it("renders the full curl-bash command for claude", () => {
    const out = formatGuestAgentMissingComment(
      "claude",
      "test-vm",
      "curl -fsSL https://claude.ai/install.sh | bash",
    );
    expect(out).toContain("curl -fsSL https://claude.ai/install.sh | bash");
  });
});
