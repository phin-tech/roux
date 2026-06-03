import { render, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NewSessionDialog from "../NewSessionDialog.svelte";
import { DEFAULT_SETTINGS } from "$lib/types";
import { settings } from "$lib/stores/settings";
import { resetProfileRegistry, setUserProfiles } from "$lib/panes/profiles";

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
  log: vi.fn(),
  logError: vi.fn(),
}));
vi.mock("$lib/tauri", () => ({
  checkGhInstalled: vi.fn().mockResolvedValue(false),
  checkIsGitRepo: vi.fn().mockResolvedValue(false),
  cloneRepo: vi.fn(),
  createSessionShell: vi.fn(),
  detachPty: vi.fn().mockResolvedValue(undefined),
  documentAttach: vi.fn(),
  documentGet: vi.fn(),
  documentList: vi.fn(),
  fetchPrBranch: vi.fn(),
  gitInit: vi.fn(),
  killPty: vi.fn().mockResolvedValue(undefined),
  killSession: vi.fn(),
  listAllPtys: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  listWorktrees: vi.fn().mockResolvedValue([]),
  lookupPr: vi.fn(),
  readFromSession: vi.fn(),
  spawnShell: vi.fn().mockResolvedValue(undefined),
  workItemCreate: vi.fn(),
  workItemDecisionsList: vi.fn().mockResolvedValue([]),
  workItemDecisionResolve: vi.fn(),
  workItemDelete: vi.fn(),
  workItemList: vi.fn().mockResolvedValue([]),
  workItemMove: vi.fn(),
  workItemPlan: vi.fn(),
  workItemReviewAccept: vi.fn(),
  workItemRunsList: vi.fn().mockResolvedValue([]),
  workItemRunStop: vi.fn(),
  workItemStart: vi.fn(),
  workItemUpdate: vi.fn(),
  writeToSession: vi.fn(),
}));
vi.mock("$lib/bindings", () => ({
  commands: {
    cmdDetectWorktrunk: vi.fn().mockResolvedValue({
      binaryPath: null,
      version: null,
      hasConfig: false,
    }),
  },
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

describe("NewSessionDialog profile defaults", () => {
  beforeEach(() => {
    resetProfileRegistry();
    setUserProfiles([...profiles]);
    settings.set({ ...DEFAULT_SETTINGS, defaultAgentProfile: "claude" });
  });

  it("reseeds the selected profile from the current default agent when opened", async () => {
    const onclose = vi.fn();
    const { rerender } = render(NewSessionDialog, { visible: false, onclose });

    settings.set({ ...DEFAULT_SETTINGS, defaultAgentProfile: "codex" });
    await rerender({ visible: true, onclose });

    await waitFor(() => {
      expect(
        (
          document.getElementById(
            "new-session-profile",
          ) as HTMLInputElement | null
        )?.value,
      ).toBe("Codex (user)");
    });
  });
});
