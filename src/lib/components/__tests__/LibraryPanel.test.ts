import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { writable } from "svelte/store";
import { beforeEach, describe, expect, it, vi } from "vitest";
import LibraryPanel from "../LibraryPanel.svelte";
import {
  listLibraryItems,
  listLibrarySources,
  getLibrarySourceStatuses,
  readLibraryItem,
  renderLibraryPrompt,
  setLibrarySources,
  writeToSession,
} from "$lib/tauri";
import { requestLibraryVariables } from "$lib/stores/libraryVariablePrompt";
import { LIBRARY_PROMPT_DRAG_MIME } from "$lib/library/drag";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("$lib/tauri", () => ({
  cloneLibrarySource: vi.fn(),
  getLibrarySourceStatuses: vi.fn(),
  listLibraryItems: vi.fn(),
  listLibrarySources: vi.fn(),
  readLibraryItem: vi.fn(),
  renderLibraryPrompt: vi.fn(),
  setLibrarySources: vi.fn(),
  syncLibrarySource: vi.fn(),
  writeToSession: vi.fn(),
}));

vi.mock("$lib/stores/sessions", () => ({
  activeSession: writable({ id: "session-1", repoRoot: "/repo" }),
}));

vi.mock("$lib/stores/libraryWindow", () => ({
  openLibraryEdit: vi.fn(),
  openLibraryNew: vi.fn(),
  openLibraryWindow: vi.fn(),
}));

vi.mock("$lib/stores/libraryVariablePrompt", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/stores/libraryVariablePrompt")>()),
  requestLibraryVariables: vi.fn(),
}));

vi.mock("$lib/panes/focus", () => ({
  focusedPaneId: writable(null),
}));

vi.mock("$lib/panes/instances", () => ({
  paneInstances: writable(new Map()),
  getAttachedPtyId: vi.fn(),
}));

const promptItem = {
  id: "fixture.release-note",
  itemType: "prompt" as const,
  title: "Draft Release Note",
  description: null,
  tags: [],
  provider: null,
  sourceLayer: "gitRepo" as const,
  sourceId: "source-1",
  sourceLabel: "Test Library",
  sourcePath: "/source/.roux/library/prompts/release.md",
  overriddenPaths: [],
  variables: [
    {
      name: "feature",
      label: "Feature",
      default: null,
      required: true,
      valueType: "string" as const,
      options: [],
    },
  ],
};

describe("LibraryPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listLibraryItems).mockResolvedValue([promptItem]);
    vi.mocked(listLibrarySources).mockResolvedValue([]);
    vi.mocked(getLibrarySourceStatuses).mockResolvedValue([]);
    vi.mocked(readLibraryItem).mockResolvedValue({
      item: promptItem,
      body: "Release {{ feature }}",
    });
    vi.mocked(requestLibraryVariables).mockResolvedValue({ feature: "Library sync" });
    vi.mocked(renderLibraryPrompt).mockResolvedValue({
      itemId: promptItem.id,
      content: "Release Library sync",
    });
    vi.mocked(writeToSession).mockResolvedValue(undefined);
  });

  it("sends an item from the sidebar row with the same variable flow", async () => {
    render(LibraryPanel, {
      props: {
        visible: true,
        onclose: vi.fn(),
      },
    });

    const sendButton = await screen.findByRole("button", {
      name: "Send Draft Release Note",
    });
    await fireEvent.click(sendButton);

    await waitFor(() => {
      expect(readLibraryItem).toHaveBeenCalledWith("fixture.release-note", "session-1");
      expect(requestLibraryVariables).toHaveBeenCalledWith({
        title: "Draft Release Note",
        variables: promptItem.variables,
        initialValues: {},
      });
      expect(renderLibraryPrompt).toHaveBeenCalledWith({
        itemId: "fixture.release-note",
        sessionId: "session-1",
        variables: { feature: "Library sync" },
      });
      expect(writeToSession).toHaveBeenCalledWith("session-1", "Release Library sync\r");
    });
  });

  it("focuses the library filter when the pane becomes visible", async () => {
    const { rerender } = render(LibraryPanel, {
      props: {
        visible: false,
        onclose: vi.fn(),
      },
    });

    await rerender({
      visible: true,
      onclose: vi.fn(),
    });

    const filter = await screen.findByPlaceholderText("Filter library...");
    await waitFor(() => {
      expect(document.activeElement).toBe(filter);
    });
  });

  it("marks prompt rows as draggable and writes prompt drag metadata", async () => {
    render(LibraryPanel, {
      props: {
        visible: true,
        onclose: vi.fn(),
      },
    });

    await screen.findByText("Draft Release Note");
    const row = screen.getByText("Draft Release Note").closest("li")?.querySelector("[draggable='true']");
    expect(row).toBeTruthy();

    const values = new Map<string, string>();
    const dataTransfer = {
      effectAllowed: "uninitialized",
      setData: vi.fn((type: string, value: string) => values.set(type, value)),
    } as unknown as DataTransfer;

    await fireEvent.dragStart(row!, { dataTransfer });

    expect(dataTransfer.effectAllowed).toBe("copy");
    expect(values.get(LIBRARY_PROMPT_DRAG_MIME)).toBe(
      JSON.stringify({
        itemId: "fixture.release-note",
        title: "Draft Release Note",
      }),
    );
  });

  it("sanitizes selected markdown previews before injecting html", async () => {
    vi.mocked(readLibraryItem).mockResolvedValue({
      item: promptItem,
      body: "# Safe\n<img src=x onerror=\"alert(1)\"><script>alert(2)</script>",
    });

    render(LibraryPanel, {
      props: {
        visible: true,
        onclose: vi.fn(),
      },
    });

    await fireEvent.click(await screen.findByText("Draft Release Note"));

    await waitFor(() => {
      expect(document.body.innerHTML).toContain("<h1>Safe</h1>");
      expect(document.body.innerHTML).not.toContain("onerror");
      expect(document.body.innerHTML).not.toContain("<script>");
    });
  });

  it("refreshes the selected item body when the list still contains the same id", async () => {
    vi.mocked(readLibraryItem).mockResolvedValueOnce({
      item: promptItem,
      body: "Old body",
    });

    render(LibraryPanel, {
      props: {
        visible: true,
        onclose: vi.fn(),
      },
    });

    await fireEvent.click(await screen.findByText("Draft Release Note"));
    await screen.findByText("Old body");

    vi.mocked(listLibraryItems).mockResolvedValueOnce([{ ...promptItem, title: "Draft Release Note" }]);
    vi.mocked(readLibraryItem).mockResolvedValueOnce({
      item: promptItem,
      body: "Updated body",
    });

    await fireEvent.click(screen.getByRole("button", { name: "Refresh library" }));

    await screen.findByText("Updated body");
    expect(screen.queryByText("Old body")).toBeNull();
    expect(readLibraryItem).toHaveBeenCalledTimes(2);
  });

  it("shows an error and restores sources when saving sources fails", async () => {
    const source = {
      id: "source-1",
      kind: "gitRepo" as const,
      name: "Team Library",
      enabled: true,
      order: 0,
      path: null,
      url: "https://example.com/team/library.git",
      branch: "main",
    };
    vi.mocked(listLibrarySources).mockResolvedValue([source]);
    vi.mocked(setLibrarySources).mockRejectedValue(new Error("could not save sources"));

    render(LibraryPanel, {
      props: {
        visible: true,
        onclose: vi.fn(),
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: /Sources/ }));
    await screen.findByText("Team Library");
    await fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    await screen.findByText("could not save sources");
    expect(screen.getByText("Team Library")).toBeTruthy();
  });

  it("uses typed selected-variable controls and blocks invalid values", async () => {
    const typedItem = {
      ...promptItem,
      variables: [
        {
          name: "tone",
          label: "Tone",
          default: "friendly",
          required: true,
          valueType: "select" as const,
          options: ["friendly", "direct"],
        },
        {
          name: "count",
          label: "Count",
          default: null,
          required: true,
          valueType: "int" as const,
          options: [],
        },
      ],
    };
    vi.mocked(listLibraryItems).mockResolvedValue([typedItem]);
    vi.mocked(readLibraryItem).mockResolvedValue({
      item: typedItem,
      body: "{{ tone }} {{ count }}",
    });

    render(LibraryPanel, {
      props: {
        visible: true,
        onclose: vi.fn(),
      },
    });

    await fireEvent.click(await screen.findByText("Draft Release Note"));
    const tone = await waitFor(() => document.querySelector<HTMLSelectElement>("select"));
    expect(tone).toBeTruthy();
    expect(Array.from(tone!.options).map((option) => option.value)).toEqual(["friendly", "direct"]);

    const count = document.querySelector<HTMLInputElement>("input[type='number']");
    expect(count?.step).toBe("1");
    await fireEvent.input(count!, { target: { value: "1.5" } });
    await fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(document.body.textContent).toContain("Count must be an integer.");
    });
    expect(renderLibraryPrompt).not.toHaveBeenCalled();
  });
});
