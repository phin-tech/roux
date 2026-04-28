import { describe, expect, it } from "vitest";
import { formatPathsForTerminal } from "../formatPaths";

describe("formatPathsForTerminal", () => {
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
});
