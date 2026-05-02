import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri", () => ({
  writeToSession: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
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
    expect(writes()).toEqual(["claude", "\r"]);
  });

  it("types a startup command without pressing Enter when typeOnly", async () => {
    await runProfileInPane(
      "pty-1",
      profile({ startupCommand: "bun run dev", startupBehavior: "typeOnly" }),
    );
    expect(writes()).toEqual(["bun run dev"]);
  });

  it("runs setup before startup, pressing Enter after each", async () => {
    await runProfileInPane(
      "pty-1",
      profile({
        setupCommand: "./scripts/start-mcp.sh",
        startupCommand: "claude --mcp-config ~/.mcp.json",
      }),
    );
    expect(writes()).toEqual([
      "./scripts/start-mcp.sh",
      "\r",
      "claude --mcp-config ~/.mcp.json",
      "\r",
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
    expect(writes()).toEqual(["export FOO=bar", "\r", "claude"]);
  });

  it("skips setup when it's only whitespace", async () => {
    await runProfileInPane(
      "pty-1",
      profile({ setupCommand: "   \n", startupCommand: "claude" }),
    );
    expect(writes()).toEqual(["claude", "\r"]);
  });

  it("propagates writeToSession errors so callers can surface them", async () => {
    // Previous behavior was to log-and-swallow, which silently produced
    // a dead-looking pane with no user feedback. Callers now catch and
    // decide how to surface: inline in the new-session dialog, as a
    // notification from the re-run button, etc.
    vi.mocked(writeToSession).mockRejectedValueOnce(new Error("dead pty"));
    await expect(
      runProfileInPane("pty-1", profile({ startupCommand: "claude" })),
    ).rejects.toThrow("dead pty");
  });

  describe("cwdOverride", () => {
    it("types a cd command before setup/startup", async () => {
      await runProfileInPane(
        "pty-1",
        profile({
          cwdOverride: "/tmp/workspace",
          startupCommand: "claude",
        }),
      );
      expect(writes()).toEqual(["cd '/tmp/workspace'", "\r", "claude", "\r"]);
    });

    it("shell-escapes paths containing spaces and single quotes", async () => {
      // Single quotes in shell strings require the 'foo'\''bar' dance;
      // without that a path like `/tmp/it's mine` would break out of the
      // quoted string and let the remainder be interpreted as commands.
      await runProfileInPane(
        "pty-1",
        profile({ cwdOverride: "/tmp/it's my dir" }),
      );
      expect(writes()).toEqual(["cd '/tmp/it'\\''s my dir'", "\r"]);
    });

    it("skips cd for an empty or whitespace-only cwdOverride", async () => {
      await runProfileInPane(
        "pty-1",
        profile({ cwdOverride: "   ", startupCommand: "claude" }),
      );
      expect(writes()).toEqual(["claude", "\r"]);
    });
  });

  describe("env", () => {
    it("emits an export per entry before setup/startup", async () => {
      await runProfileInPane(
        "pty-1",
        profile({
          env: { FOO: "bar", BAZ: "qux" },
          startupCommand: "claude",
        }),
      );
      // BTreeMap serialization is lexicographic, so BAZ comes first.
      // We don't hard-assert order in this test because Object iteration
      // in JS is insertion-ordered; just check both are present and
      // precede the startup command.
      const out = writes();
      const startupIdx = out.indexOf("claude");
      expect(startupIdx).toBeGreaterThan(-1);
      expect(out.slice(0, startupIdx)).toEqual(
        expect.arrayContaining(["export FOO='bar'", "export BAZ='qux'", "\r"]),
      );
    });

    it("shell-escapes env values containing single quotes and metacharacters", async () => {
      await runProfileInPane(
        "pty-1",
        profile({ env: { MSG: "it's; $(whoami)" } }),
      );
      expect(writes()).toEqual([
        "export MSG='it'\\''s; $(whoami)'",
        "\r",
      ]);
    });

    it("skips env entries with invalid shell variable names", async () => {
      // Names must match [A-Za-z_][A-Za-z0-9_]*. `1BAD` and `WITH-DASH`
      // would silently break the exported line if we didn't filter them.
      await runProfileInPane(
        "pty-1",
        profile({
          env: { "1BAD": "x", "WITH-DASH": "y", GOOD: "z" },
        }),
      );
      expect(writes()).toEqual(["export GOOD='z'", "\r"]);
    });
  });

  it("applies cwdOverride → env → setup → startup in order", async () => {
    await runProfileInPane(
      "pty-1",
      profile({
        cwdOverride: "/work",
        env: { API: "https://api" },
        setupCommand: "./seed.sh",
        startupCommand: "claude",
      }),
    );
    expect(writes()).toEqual([
      "cd '/work'",
      "\r",
      "export API='https://api'",
      "\r",
      "./seed.sh",
      "\r",
      "claude",
      "\r",
    ]);
  });
});
