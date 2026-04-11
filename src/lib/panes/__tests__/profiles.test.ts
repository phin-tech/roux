import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/bindings", () => ({
  commands: {
    getBuiltinProfiles: vi.fn(),
  },
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

import {
  loadBuiltinProfiles,
  setUserProfiles,
  profileRegistry,
  profileList,
  resolveProfileRef,
  resetProfileRegistry,
} from "../profiles";
import type { SpawnProfile } from "../profiles";
import { commands } from "$lib/bindings";

function builtin(id: string, name: string, extras: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id,
    name,
    source: "builtin",
    ...extras,
  };
}

function user(id: string, name: string, extras: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id,
    name,
    source: "user",
    ...extras,
  };
}

describe("profile registry", () => {
  beforeEach(() => {
    resetProfileRegistry();
    vi.mocked(commands.getBuiltinProfiles).mockReset();
  });

  it("loadBuiltinProfiles populates the built-in segment from the Tauri command", async () => {
    vi.mocked(commands.getBuiltinProfiles).mockResolvedValue([
      builtin("claude", "Claude", { provider: "claude", startupCommand: "claude" }),
      builtin("plain-shell", "Plain shell"),
    ]);

    await loadBuiltinProfiles();

    const registry = get(profileRegistry);
    expect(registry.has("claude")).toBe(true);
    expect(registry.has("plain-shell")).toBe(true);
    expect(registry.get("claude")?.provider).toBe("claude");
  });

  it("loadBuiltinProfiles clears the built-in segment when the Tauri call throws", async () => {
    // Seed a profile so we can observe it being cleared.
    vi.mocked(commands.getBuiltinProfiles).mockResolvedValueOnce([builtin("stale", "Stale")]);
    await loadBuiltinProfiles();
    expect(get(profileRegistry).has("stale")).toBe(true);

    vi.mocked(commands.getBuiltinProfiles).mockRejectedValue(new Error("IPC down"));
    await loadBuiltinProfiles();
    expect(get(profileRegistry).has("stale")).toBe(false);
  });

  it("setUserProfiles adds user entries and force-stamps source: 'user'", () => {
    // A would-be forged entry claiming to be built-in.
    setUserProfiles([user("my-dev-server", "Dev server")]);
    // Simulate a malicious JSON blob claiming builtin; the stamp must override.
    setUserProfiles([
      { id: "sneaky", name: "Sneaky", source: "builtin" } as unknown as SpawnProfile,
    ]);

    const registry = get(profileRegistry);
    expect(registry.get("sneaky")?.source).toBe("user");
  });

  it("user profiles override built-ins on id collision", async () => {
    vi.mocked(commands.getBuiltinProfiles).mockResolvedValue([
      builtin("claude", "Claude (built-in)", { startupCommand: "claude" }),
    ]);
    await loadBuiltinProfiles();
    setUserProfiles([user("claude", "Claude (mine)", { startupCommand: "claude --mcp" })]);

    const profile = get(profileRegistry).get("claude");
    expect(profile?.name).toBe("Claude (mine)");
    expect(profile?.startupCommand).toBe("claude --mcp");
    expect(profile?.source).toBe("user");
  });

  it("profileList returns built-ins first, then users in insertion order", async () => {
    vi.mocked(commands.getBuiltinProfiles).mockResolvedValue([
      builtin("claude", "Claude"),
      builtin("plain-shell", "Plain shell"),
    ]);
    await loadBuiltinProfiles();
    setUserProfiles([user("dev-server", "Dev server")]);

    const list = get(profileList);
    const ids = list.map((p) => p.id);
    expect(ids).toEqual(["claude", "plain-shell", "dev-server"]);
  });

  it("resolveProfileRef returns the captured profile for inline refs", () => {
    const inline = user("inline-xyz", "Custom task", { startupCommand: "echo hi" });
    const resolved = resolveProfileRef({ kind: "inline", profile: inline });
    expect(resolved).toEqual(inline);
  });

  it("resolveProfileRef returns the registry hit for registered refs", async () => {
    vi.mocked(commands.getBuiltinProfiles).mockResolvedValue([builtin("claude", "Claude")]);
    await loadBuiltinProfiles();

    const resolved = resolveProfileRef({ kind: "registered", id: "claude" });
    expect(resolved?.id).toBe("claude");
  });

  it("resolveProfileRef returns null when a registered profile no longer exists", () => {
    const resolved = resolveProfileRef({ kind: "registered", id: "gone" });
    expect(resolved).toBeNull();
  });

  it("resolveProfileRef returns null for undefined refs", () => {
    expect(resolveProfileRef(undefined)).toBeNull();
  });
});
