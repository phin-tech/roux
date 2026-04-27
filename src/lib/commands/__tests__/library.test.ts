import { get, writable } from "svelte/store";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { registry } from "../registry";
import { registerLibraryCommands } from "../library";
import { listLibraryItems } from "$lib/tauri";

vi.mock("$lib/tauri", () => ({
  listLibraryItems: vi.fn(),
  notificationsPush: vi.fn(),
  readLibraryItem: vi.fn(),
  renderLibraryPrompt: vi.fn(),
  writeToSession: vi.fn(),
}));

vi.mock("$lib/stores/sessions", () => ({
  activeSession: writable(null),
}));

vi.mock("$lib/panes/focus", () => ({
  focusedPaneId: writable(null),
}));

vi.mock("$lib/panes/instances", () => ({
  paneInstances: writable(new Map()),
  getAttachedPtyId: vi.fn(),
}));

vi.mock("$lib/stores/libraryWindow", () => ({
  openLibraryWindow: vi.fn(),
}));

vi.mock("$lib/stores/libraryVariablePrompt", () => ({
  requestLibraryVariables: vi.fn(),
}));

vi.mock("$lib/logging", () => ({
  logError: vi.fn(),
}));

const promptItem = {
  id: "review.diff",
  itemType: "prompt" as const,
  title: "Review Diff",
  description: null,
  tags: [],
  provider: null,
  sourceLayer: "global" as const,
  sourceId: null,
  sourceLabel: "Global",
  sourcePath: "/tmp/review.md",
  overriddenPaths: [],
  variables: [],
};

const skillItem = {
  ...promptItem,
  id: "rust.errors",
  itemType: "skill" as const,
  title: "Rust Errors",
  sourcePath: "/tmp/rust.md",
};

describe("library commands", () => {
  beforeEach(() => {
    for (const id of [
      "library.search-prompts",
      "library.search-skills",
      "library.copy-prompt-to-clipboard",
      "library.copy-skill-to-clipboard",
      "library.open-manager",
      "library.send-to-active-pane",
      "library.copy-to-clipboard",
    ]) {
      registry.unregister(id);
    }
    vi.mocked(listLibraryItems).mockReset();
  });

  it("registers separate prompt and skill search commands", async () => {
    vi.mocked(listLibraryItems).mockResolvedValue([promptItem, skillItem]);
    registerLibraryCommands();

    const commands = get(writable(registry.getAvailable().map((cmd) => cmd.id)));

    expect(commands).toContain("library.search-prompts");
    expect(commands).toContain("library.search-skills");
    expect(commands).not.toContain("library.send-to-active-pane");

    const promptItems = await registry.get("library.search-prompts")!.getItems!();
    const skillItems = await registry.get("library.search-skills")!.getItems!();

    expect(promptItems.map((item) => item.label)).toEqual(["Review Diff"]);
    expect(skillItems.map((item) => item.label)).toEqual(["Rust Errors"]);
    expect(registry.get("library.search-prompts")?.inputPlaceholder).toBe("Search prompts...");
    expect(registry.get("library.search-skills")?.inputPlaceholder).toBe("Search skills...");
  });
});
