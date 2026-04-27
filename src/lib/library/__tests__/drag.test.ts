import { describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { LibraryItem } from "$lib/tauri";
import {
  LIBRARY_PROMPT_DRAG_MIME,
  clearDraggedLibraryPrompt,
  draggedLibraryPrompt,
  hasLibraryPromptDragData,
  libraryPromptDragPayload,
  readLibraryPromptDragData,
  writeLibraryPromptDragData,
} from "../drag";

function item(overrides: Partial<LibraryItem> = {}): LibraryItem {
  return {
    id: "fixture.release-note",
    itemType: "prompt",
    title: "Draft Release Note",
    description: null,
    tags: [],
    provider: null,
    sourceLayer: "gitRepo",
    sourceId: "source-1",
    sourceLabel: "Test Library",
    sourcePath: "/source/.roux/library/prompts/release.md",
    overriddenPaths: [],
    variables: [],
    ...overrides,
  };
}

function dataTransfer() {
  const values = new Map<string, string>();
  return {
    effectAllowed: "uninitialized",
    dropEffect: "none",
    get types() {
      return Array.from(values.keys());
    },
    setData: vi.fn((type: string, value: string) => values.set(type, value)),
    getData: vi.fn((type: string) => values.get(type) ?? ""),
  } as unknown as DataTransfer;
}

describe("Library prompt drag data", () => {
  it("creates payloads only for prompts", () => {
    expect(libraryPromptDragPayload(item())).toEqual({
      itemId: "fixture.release-note",
      title: "Draft Release Note",
    });
    expect(libraryPromptDragPayload(item({ itemType: "skill" }))).toBeNull();
  });

  it("writes and reads prompt drag metadata", () => {
    const transfer = dataTransfer();
    clearDraggedLibraryPrompt();

    expect(writeLibraryPromptDragData(transfer, item())).toBe(true);

    expect(transfer.effectAllowed).toBe("copy");
    expect(transfer.setData).toHaveBeenCalledWith(
      LIBRARY_PROMPT_DRAG_MIME,
      JSON.stringify({
        itemId: "fixture.release-note",
        title: "Draft Release Note",
      }),
    );
    expect(readLibraryPromptDragData(transfer)).toEqual({
      itemId: "fixture.release-note",
      title: "Draft Release Note",
    });
    expect(hasLibraryPromptDragData(transfer)).toBe(true);
    expect(get(draggedLibraryPrompt)).toEqual({
      itemId: "fixture.release-note",
      title: "Draft Release Note",
    });
  });

  it("ignores malformed drag metadata", () => {
    const transfer = dataTransfer();
    transfer.setData(LIBRARY_PROMPT_DRAG_MIME, "{");

    expect(readLibraryPromptDragData(transfer)).toBeNull();
  });
});
