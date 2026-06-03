import { render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NewProjectDialog from "../NewProjectDialog.svelte";
import { DEFAULT_SETTINGS } from "$lib/types";
import { resetProfileRegistry, setUserProfiles } from "$lib/panes/profiles";
import { settings } from "$lib/stores/settings";

if (typeof Element !== "undefined" && !Element.prototype.animate) {
  Element.prototype.animate = () =>
    ({
      cancel() {},
      play() {},
      pause() {},
      finished: Promise.resolve(),
      onfinish: null,
      currentTime: 0,
      playState: "finished",
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as Animation;
}

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("$lib/logging", () => ({
  logError: vi.fn(),
}));
vi.mock("$lib/stores/projects", () => ({
  createProjectFull: vi.fn(),
  removeProject: vi.fn(),
  updateProject: vi.fn(),
}));
vi.mock("$lib/sessions/spawnBlueprint", () => ({
  spawnBlueprintForProject: vi.fn(),
}));
vi.mock("$lib/tauri", () => ({
  listGitReposInRoots: vi.fn().mockResolvedValue([]),
}));

const profiles = [
  {
    id: "claude",
    name: "Claude",
    setupCommand: null,
    startupCommand: "claude",
    startupBehavior: "autoRun",
    env: null,
    cwdOverride: null,
    icon: null,
    provider: "claude",
    source: "user",
  },
  {
    id: "codex",
    name: "Codex",
    setupCommand: null,
    startupCommand: "codex",
    startupBehavior: "autoRun",
    env: null,
    cwdOverride: null,
    icon: null,
    provider: "codex",
    source: "user",
  },
] as const;

describe("NewProjectDialog profile defaults", () => {
  beforeEach(() => {
    resetProfileRegistry();
    setUserProfiles([...profiles]);
    settings.set({ ...DEFAULT_SETTINGS, defaultAgentProfile: "claude" });
  });

  it("updates the generated-session default when the preference changes while mounted", async () => {
    render(NewProjectDialog, { visible: true, onclose: vi.fn() });

    expect(screen.getByText("claude (default)")).toBeDefined();

    settings.set({ ...DEFAULT_SETTINGS, defaultAgentProfile: "codex" });

    await waitFor(() => {
      expect(screen.getByText("codex (default)")).toBeDefined();
    });
  });
});
