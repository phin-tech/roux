import { describe, expect, it } from "vitest";

import { buildSubmitPayload } from "../multiLineEditor";

describe("buildSubmitPayload", () => {
  it("prefixes shell payloads with Ctrl+E + Ctrl+U and wraps in bracketed paste", () => {
    expect(buildSubmitPayload("echo hi", "shell")).toBe(
      "\x05\x15\x1b[200~echo hi\x1b[201~",
    );
  });

  it("skips the prompt-clear bytes for Claude Code panes", () => {
    expect(buildSubmitPayload("echo hi", "claude")).toBe(
      "\x1b[200~echo hi\x1b[201~",
    );
  });

  it("preserves multi-line content inside the bracketed-paste wrapper", () => {
    const multiline = "echo one\necho two\necho three";
    expect(buildSubmitPayload(multiline, "shell")).toBe(
      `\x05\x15\x1b[200~${multiline}\x1b[201~`,
    );
  });

  it("never appends Enter — user reviews and submits in the terminal", () => {
    const payload = buildSubmitPayload("dangerous --force", "shell");
    // Enter is \r (CR) or \n (LF); neither may appear at the end.
    expect(payload.endsWith("\r")).toBe(false);
    expect(payload.endsWith("\n")).toBe(false);
    // The final 6 chars must be the bracketed-paste end marker.
    expect(payload.endsWith("\x1b[201~")).toBe(true);
  });

  it("handles an empty editor without corrupting the markers", () => {
    expect(buildSubmitPayload("", "shell")).toBe("\x05\x15\x1b[200~\x1b[201~");
    expect(buildSubmitPayload("", "claude")).toBe("\x1b[200~\x1b[201~");
  });
});
