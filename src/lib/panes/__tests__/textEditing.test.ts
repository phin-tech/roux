import { describe, expect, it } from "vitest";

import {
  clearBuffer,
  clearSelectedLines,
  copyAndClearCurrentLine,
  deleteToLineEnd,
  deleteToLineStart,
  deleteWordLeft,
  insertAtSelection,
} from "../textEditing";

describe("textarea editing helpers", () => {
  it("inserts text at the cursor", () => {
    expect(
      insertAtSelection(
        { value: "echo hi", selectionStart: 4, selectionEnd: 4 },
        "\n",
      ),
    ).toEqual({
      value: "echo\n hi",
      selectionStart: 5,
      selectionEnd: 5,
    });
  });

  it("replaces the selected range when inserting text", () => {
    expect(
      insertAtSelection(
        { value: "echo hello", selectionStart: 5, selectionEnd: 10 },
        "\n",
      ),
    ).toEqual({
      value: "echo \n",
      selectionStart: 6,
      selectionEnd: 6,
    });
  });

  it("clears the whole buffer", () => {
    expect(
      clearBuffer({ value: "one\ntwo", selectionStart: 3, selectionEnd: 3 }),
    ).toEqual({
      value: "",
      selectionStart: 0,
      selectionEnd: 0,
    });
  });

  it("copies and clears the current middle line", () => {
    expect(
      copyAndClearCurrentLine({
        value: "one\ntwo three\nfour",
        selectionStart: 6,
        selectionEnd: 6,
      }),
    ).toEqual({
      value: "one\n\nfour",
      selectionStart: 4,
      selectionEnd: 4,
      clipboardText: "two three",
    });
  });

  it("copies and clears the only line", () => {
    expect(
      copyAndClearCurrentLine({
        value: "echo hi",
        selectionStart: 7,
        selectionEnd: 7,
      }),
    ).toEqual({
      value: "",
      selectionStart: 0,
      selectionEnd: 0,
      clipboardText: "echo hi",
    });
  });

  it("clears every line touched by a selection", () => {
    expect(
      clearSelectedLines({
        value: "alpha\nbeta\ngamma\ndelta",
        selectionStart: 7,
        selectionEnd: 14,
      }),
    ).toEqual({
      value: "alpha\n\n\ndelta",
      selectionStart: 6,
      selectionEnd: 6,
    });
  });

  it("clears the current line when no text is selected", () => {
    expect(
      clearSelectedLines({
        value: "alpha\nbeta\ngamma",
        selectionStart: 8,
        selectionEnd: 8,
      }),
    ).toEqual({
      value: "alpha\n\ngamma",
      selectionStart: 6,
      selectionEnd: 6,
    });
  });

  it("deletes the word to the left plus trailing whitespace", () => {
    expect(
      deleteWordLeft({
        value: "echo hello   ",
        selectionStart: 13,
        selectionEnd: 13,
      }),
    ).toEqual({
      value: "echo ",
      selectionStart: 5,
      selectionEnd: 5,
    });
  });

  it("deletes a selected range instead of expanding to a word", () => {
    expect(
      deleteWordLeft({
        value: "echo hello",
        selectionStart: 5,
        selectionEnd: 10,
      }),
    ).toEqual({
      value: "echo ",
      selectionStart: 5,
      selectionEnd: 5,
    });
  });

  it("deletes to the start of the current line", () => {
    expect(
      deleteToLineStart({
        value: "one\ntwo three",
        selectionStart: 8,
        selectionEnd: 8,
      }),
    ).toEqual({
      value: "one\nthree",
      selectionStart: 4,
      selectionEnd: 4,
    });
  });

  it("deletes to the end of the current line", () => {
    expect(
      deleteToLineEnd({
        value: "one\ntwo three\nfour",
        selectionStart: 8,
        selectionEnd: 8,
      }),
    ).toEqual({
      value: "one\ntwo \nfour",
      selectionStart: 8,
      selectionEnd: 8,
    });
  });
});
