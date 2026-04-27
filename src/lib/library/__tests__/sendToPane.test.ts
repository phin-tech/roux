import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  readLibraryItem,
  renderLibraryPrompt,
  writeToSession,
  type LibraryRead,
} from "$lib/tauri";
import { requestLibraryVariables } from "$lib/stores/libraryVariablePrompt";
import { LIBRARY_PROMPT_DRAG_MIME } from "../drag";
import { sendDroppedLibraryPromptToPty } from "../sendToPane";

vi.mock("$lib/tauri", () => ({
  readLibraryItem: vi.fn(),
  renderLibraryPrompt: vi.fn(),
  writeToSession: vi.fn(),
}));

vi.mock("$lib/stores/libraryVariablePrompt", () => ({
  requestLibraryVariables: vi.fn(),
}));

function transfer(itemId = "fixture.release-note") {
  return {
    getData: vi.fn((type: string) => {
      if (type !== LIBRARY_PROMPT_DRAG_MIME) return "";
      return JSON.stringify({ itemId, title: "Draft Release Note" });
    }),
  } as unknown as DataTransfer;
}

const promptRead: LibraryRead = {
  item: {
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
    variables: [
      {
        name: "feature",
        label: "Feature",
        default: null,
        required: true,
        valueType: "string",
        options: [],
      },
    ],
  },
  body: "Release {{ feature }}",
};

describe("sendDroppedLibraryPromptToPty", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(readLibraryItem).mockResolvedValue(promptRead);
    vi.mocked(requestLibraryVariables).mockResolvedValue({ feature: "drag and drop" });
    vi.mocked(renderLibraryPrompt).mockResolvedValue({
      itemId: "fixture.release-note",
      content: "Release drag and drop",
    });
    vi.mocked(writeToSession).mockResolvedValue(undefined);
  });

  it("renders dropped prompts and submits them into the target pty", async () => {
    await expect(sendDroppedLibraryPromptToPty(transfer(), "pty-2", "session-1")).resolves.toBe(true);

    expect(readLibraryItem).toHaveBeenCalledWith("fixture.release-note", "session-1");
    expect(requestLibraryVariables).toHaveBeenCalledWith({
      title: "Draft Release Note",
      variables: promptRead.item.variables,
      initialValues: {},
    });
    expect(renderLibraryPrompt).toHaveBeenCalledWith({
      itemId: "fixture.release-note",
      sessionId: "session-1",
      variables: { feature: "drag and drop" },
    });
    expect(writeToSession).toHaveBeenCalledWith("pty-2", "Release drag and drop\r");
  });

  it("does not write when the drop has no target pty", async () => {
    await expect(sendDroppedLibraryPromptToPty(transfer(), null, "session-1")).resolves.toBe(false);

    expect(writeToSession).not.toHaveBeenCalled();
  });
});
