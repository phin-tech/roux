import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { formatPathsForTerminal } from "../formatPaths";

function stubPlatform(value: string) {
  Object.defineProperty(window.navigator, "platform", {
    configurable: true,
    get: () => value,
  });
}

describe("formatPathsForTerminal (POSIX)", () => {
  beforeEach(() => stubPlatform("MacIntel"));
  afterEach(() => stubPlatform(""));

  it("returns empty string for empty input", () => {
    expect(formatPathsForTerminal([])).toBe("");
  });

  it("returns a path with safe chars unquoted", () => {
    expect(formatPathsForTerminal(["/tmp/file.txt"])).toBe("/tmp/file.txt");
  });

  it("preserves common safe punctuation without quoting", () => {
    expect(formatPathsForTerminal(["/path/to/file-1_v2.tar.gz"])).toBe(
      "/path/to/file-1_v2.tar.gz",
    );
  });

  it("single-quotes paths containing spaces", () => {
    expect(formatPathsForTerminal(["/tmp/with space.txt"])).toBe(
      "'/tmp/with space.txt'",
    );
  });

  it("escapes embedded single quotes via POSIX trick", () => {
    expect(formatPathsForTerminal(["/tmp/it's.txt"])).toBe(
      "'/tmp/it'\\''s.txt'",
    );
  });

  it("quotes paths containing shell metacharacters", () => {
    expect(formatPathsForTerminal(["/tmp/a$b.txt"])).toBe("'/tmp/a$b.txt'");
    expect(formatPathsForTerminal(["/tmp/a;b.txt"])).toBe("'/tmp/a;b.txt'");
    expect(formatPathsForTerminal(["/tmp/a*b.txt"])).toBe("'/tmp/a*b.txt'");
  });

  it("joins multiple paths with a single space", () => {
    expect(formatPathsForTerminal(["/a/b", "/c d"])).toBe("/a/b '/c d'");
  });

  it("quotes empty-string entries (defensive)", () => {
    expect(formatPathsForTerminal([""])).toBe("''");
  });

  it("escapes embedded newlines so they cannot fire as Enter in the PTY", () => {
    expect(formatPathsForTerminal(["/tmp/a\nb"])).toBe("'/tmp/a\\nb'");
  });

  it("escapes embedded carriage returns and tabs", () => {
    expect(formatPathsForTerminal(["/tmp/a\rb"])).toBe("'/tmp/a\\rb'");
    expect(formatPathsForTerminal(["/tmp/a\tb"])).toBe("'/tmp/a\\tb'");
  });

  it("escapes other control characters as hex", () => {
    expect(formatPathsForTerminal(["/tmp/a\x01b"])).toBe("'/tmp/a\\x01b'");
    expect(formatPathsForTerminal(["/tmp/a\x7fb"])).toBe("'/tmp/a\\x7fb'");
  });
});

describe("formatPathsForTerminal (Windows)", () => {
  beforeEach(() => stubPlatform("Win32"));
  afterEach(() => stubPlatform(""));

  it("leaves a backslashed Windows path unquoted when otherwise safe", () => {
    expect(formatPathsForTerminal(["C:\\Users\\sam\\file.txt"])).toBe(
      "C:\\Users\\sam\\file.txt",
    );
  });

  it("double-quotes Windows paths with spaces", () => {
    expect(formatPathsForTerminal(["C:\\Program Files\\app"])).toBe(
      '"C:\\Program Files\\app"',
    );
  });

  it("escapes embedded double quotes", () => {
    expect(formatPathsForTerminal(['C:\\a"b'])).toBe('"C:\\a\\"b"');
  });

  it("still strips control characters on Windows", () => {
    const out = formatPathsForTerminal(["C:\\a\nb"]);
    expect(out).not.toContain("\n");
    expect(out).toContain("\\n");
  });
});
