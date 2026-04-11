import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri", () => ({
  writeToSession: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

import { runProfileInPane } from "../profileRunner";
import { writeToSession } from "$lib/tauri";
import type { SpawnProfile } from "../profiles";

function profile(overrides: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id: "test",
    name: "Test",
    source: "builtin",
    ...overrides,
  };
}

function writes(): string[] {
  return vi.mocked(writeToSession).mock.calls.map(([, data]) => data);
}

describe("runProfileInPane", () => {
  beforeEach(() => {
    vi.mocked(writeToSession).mockClear();
  });

  it("is a no-op for profiles with neither command", async () => {
    await runProfileInPane("pty-1", profile());
    expect(writeToSession).not.toHaveBeenCalled();
  });

  it("types and auto-runs a startup command by default", async () => {
    await runProfileInPane("pty-1", profile({ startupCommand: "claude" }));
    expect(writes()).toEqual(["claude", "\n"]);
  });

  it("types a startup command without a trailing newline when typeOnly", async () => {
    await runProfileInPane(
      "pty-1",
      profile({ startupCommand: "bun run dev", startupBehavior: "typeOnly" }),
    );
    expect(writes()).toEqual(["bun run dev"]);
  });

  it("runs setup before startup, each with its own newline", async () => {
    await runProfileInPane(
      "pty-1",
      profile({
        setupCommand: "./scripts/start-mcp.sh",
        startupCommand: "claude --mcp-config ~/.mcp.json",
      }),
    );
    expect(writes()).toEqual([
      "./scripts/start-mcp.sh",
      "\n",
      "claude --mcp-config ~/.mcp.json",
      "\n",
    ]);
  });

  it("setup always auto-runs even when startupBehavior is typeOnly", async () => {
    await runProfileInPane(
      "pty-1",
      profile({
        setupCommand: "export FOO=bar",
        startupCommand: "claude",
        startupBehavior: "typeOnly",
      }),
    );
    expect(writes()).toEqual(["export FOO=bar", "\n", "claude"]);
  });

  it("skips setup when it's only whitespace", async () => {
    await runProfileInPane(
      "pty-1",
      profile({ setupCommand: "   \n", startupCommand: "claude" }),
    );
    expect(writes()).toEqual(["claude", "\n"]);
  });

  it("swallows writeToSession errors so a failed tab doesn't break callers", async () => {
    vi.mocked(writeToSession).mockRejectedValueOnce(new Error("dead pty"));
    await expect(
      runProfileInPane("pty-1", profile({ startupCommand: "claude" })),
    ).resolves.toBeUndefined();
  });
});
