import { describe, expect, it } from "vitest";

import { readPromptSnapshot } from "../promptSnapshot";

interface FakeLine {
  isWrapped: boolean;
  translateToString(trimRight?: boolean): string;
}

interface FakeBuffer {
  type: "normal" | "alternate";
  cursorY: number;
  cursorX: number;
  baseY: number;
  viewportY: number;
  length: number;
  getLine(y: number): FakeLine | undefined;
}

function line(text: string, isWrapped = false): FakeLine {
  return {
    isWrapped,
    translateToString: (trimRight?: boolean) =>
      trimRight ? text.replace(/\s+$/, "") : text,
  };
}

function buildBuffer(
  lines: FakeLine[],
  opts: { baseY?: number; cursorY: number },
): FakeBuffer {
  return {
    type: "normal",
    cursorY: opts.cursorY,
    cursorX: 0,
    baseY: opts.baseY ?? 0,
    viewportY: opts.baseY ?? 0,
    length: lines.length,
    getLine: (y) => lines[y],
  };
}

describe("readPromptSnapshot", () => {
  it("reads the current cursor line and strips a $ prompt prefix", () => {
    const buf = buildBuffer([line("$ echo hi")], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toEqual({ text: "echo hi", seeded: true });
  });

  it("strips ❯ and # prefixes", () => {
    expect(
      readPromptSnapshot(buildBuffer([line("❯ ls")], { cursorY: 0 })),
    ).toEqual({ text: "ls", seeded: true });
    expect(
      readPromptSnapshot(buildBuffer([line("# apt update")], { cursorY: 0 })),
    ).toEqual({ text: "apt update", seeded: true });
  });

  it("walks backwards through wrapped continuation lines", () => {
    const buf = buildBuffer(
      [
        line("$ echo veryveryveryveryveryveryveryveryveryvery"),
        line("longlonglonglonglonglonglong", true),
      ],
      { cursorY: 1 },
    );
    expect(readPromptSnapshot(buf)).toEqual({
      text: "echo veryveryveryveryveryveryveryveryveryverylonglonglonglonglonglonglong",
      seeded: true,
    });
  });

  it("handles scrolled buffer (baseY > 0)", () => {
    // Simulate a scrolled buffer: the cursor line is at absolute index
    // baseY + cursorY = 100 + 2 = 102.
    const lines: FakeLine[] = [];
    for (let i = 0; i < 102; i++) lines.push(line(""));
    lines.push(line("$ ls -la"));
    const buf = buildBuffer(lines, { baseY: 100, cursorY: 2 });
    expect(readPromptSnapshot(buf)).toEqual({ text: "ls -la", seeded: true });
  });

  it("returns null when the cursor line does not exist", () => {
    const buf = buildBuffer([], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toBeNull();
  });

  it("returns null when the cursor line is empty after stripping", () => {
    const buf = buildBuffer([line("$ ")], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toBeNull();
  });

  it("leaves lines without a known prefix alone", () => {
    const buf = buildBuffer([line("some raw line")], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toEqual({
      text: "some raw line",
      seeded: true,
    });
  });

  it("trims trailing whitespace from the composed result", () => {
    const buf = buildBuffer([line("$ echo hi   ")], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toEqual({ text: "echo hi", seeded: true });
  });

  it("walks FORWARD through wrapped continuations when cursor is mid-wrap", () => {
    // User typed a long command, then arrow-keyed back to an earlier row —
    // the cursor is on line 1, but the logical input continues on line 2.
    const buf = buildBuffer(
      [
        line("$ echo aaaaaaaaaaaaaaaa"),
        line("bbbbbbbbbbbbbbbb", true),
        line("ccccccccccccc", true),
      ],
      { cursorY: 1 },
    );
    expect(readPromptSnapshot(buf)).toEqual({
      text: "echo aaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbccccccccccccc",
      seeded: true,
    });
  });

  it("preserves a leading space the user typed (HISTCONTROL=ignorespace)", () => {
    // Bash's `HISTCONTROL=ignorespace` — prefixing a command with a space
    // keeps it out of history. That leading space is user-meaningful and
    // must not be stripped along with the prompt marker.
    const buf = buildBuffer([line("$  ls -la")], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toEqual({ text: " ls -la", seeded: true });
  });

  it("still strips the prompt marker when followed by a tab", () => {
    const buf = buildBuffer([line("$\techo hi")], { cursorY: 0 });
    expect(readPromptSnapshot(buf)).toEqual({ text: "echo hi", seeded: true });
  });
});
