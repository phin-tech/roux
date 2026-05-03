import { describe, expect, it } from "vitest";

import { buildSubmitPayload } from "../multiLineEditor";

describe("buildSubmitPayload", () => {
  it("clears shell input, then wraps payloads in bracketed paste", () => {
    expect(buildSubmitPayload("echo hi", "shell")).toBe(
      "\x05\x15\x1b[200~echo hi\x1b[201~",
    );
  });

  it("wraps Claude Code payloads in bracketed paste", () => {
    expect(buildSubmitPayload("echo hi", "claude")).toBe(
      "\x1b[200~echo hi\x1b[201~",
    );
  });

  it("preserves multi-line shell content inside the paste wrapper", () => {
    const multiline = "echo one\necho two\necho three";
    expect(buildSubmitPayload(multiline, "shell")).toBe(
      `\x05\x15\x1b[200~${multiline}\x1b[201~`,
    );
  });

  it("does not append Enter so submit can send Enter as a separate write", () => {
    const payload = buildSubmitPayload("dangerous --force", "shell");
    expect(payload.endsWith("\r")).toBe(false);
    expect(payload.endsWith("\n")).toBe(false);
    expect(payload).toContain("\x1b[200~");
    expect(payload.startsWith("\x05\x15")).toBe(true);
  });

  it("trims trailing editor line breaks because submit sends Enter separately", () => {
    expect(buildSubmitPayload("echo hi\n\n", "shell")).toBe(
      "\x05\x15\x1b[200~echo hi\x1b[201~",
    );
  });

  it("handles an empty editor without corrupting the markers", () => {
    expect(buildSubmitPayload("", "shell")).toBe("\x05\x15\x1b[200~\x1b[201~");
    expect(buildSubmitPayload("", "claude")).toBe("\x1b[200~\x1b[201~");
  });
});
