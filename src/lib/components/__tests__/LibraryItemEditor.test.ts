import { fireEvent, render, screen } from "@testing-library/svelte";
import { writable } from "svelte/store";
import { describe, expect, it, vi } from "vitest";
import LibraryItemEditor from "../LibraryItemEditor.svelte";

vi.mock("@codemirror/state", () => ({
  EditorState: {
    create: ({ doc }: { doc: string }) => ({
      doc: {
        length: doc.length,
        toString: () => doc,
      },
    }),
  },
}));

vi.mock("@codemirror/commands", () => ({
  defaultKeymap: [],
  history: vi.fn(() => ({})),
  historyKeymap: [],
}));

vi.mock("@codemirror/lang-markdown", () => ({
  markdown: vi.fn(() => ({})),
  markdownLanguage: {},
}));

vi.mock("@codemirror/language-data", () => ({
  languages: [],
}));

vi.mock("@codemirror/language", () => ({
  defaultHighlightStyle: {},
  syntaxHighlighting: vi.fn(() => ({})),
}));

vi.mock("@codemirror/view", () => {
  class EditorView {
    static lineWrapping = {};
    static updateListener = { of: vi.fn(() => ({})) };
    static theme = vi.fn(() => ({}));

    state: { doc: { length: number; toString: () => string } };

    constructor({ state }: { state: { doc: { length: number; toString: () => string } } }) {
      this.state = state;
    }

    dispatch = vi.fn();
    destroy = vi.fn();
  }

  return {
    EditorView,
    keymap: { of: vi.fn(() => ({})) },
  };
});

vi.mock("$lib/stores/settings", () => ({
  settings: writable({ fontSize: 14, fontFamily: "monospace" }),
}));

describe("LibraryItemEditor", () => {
  it("guards against duplicate saves while a save is pending", async () => {
    let resolveSave: () => void = () => {};
    const onsave = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveSave = resolve;
        }),
    );

    render(LibraryItemEditor, {
      props: {
        item: null,
        itemType: "prompt",
        sources: [],
        activeRepo: null,
        onsave,
        oncancel: vi.fn(),
      },
    });

    const saveButton = screen.getByRole("button", { name: "Save" });
    await fireEvent.click(saveButton);
    await fireEvent.click(saveButton);

    expect(onsave).toHaveBeenCalledTimes(1);
    expect((saveButton as HTMLButtonElement).disabled).toBe(true);

    resolveSave();
  });

  it("does not reset a new draft when the active repo changes", async () => {
    const { rerender } = render(LibraryItemEditor, {
      props: {
        item: null,
        itemType: "prompt",
        sources: [],
        activeRepo: null,
        onsave: vi.fn(),
        oncancel: vi.fn(),
      },
    });

    const title = screen.getByLabelText("Title") as HTMLInputElement;
    await fireEvent.input(title, { target: { value: "Draft prompt" } });

    await rerender({
      item: null,
      itemType: "prompt",
      sources: [],
      activeRepo: "/repo",
      onsave: vi.fn(),
      oncancel: vi.fn(),
    });

    expect(title.value).toBe("Draft prompt");
  });
});
