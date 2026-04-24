// Pure text transforms for the multi-line prompt editor's toolbar buttons.
// Each function maps an input string to its cleaned-up form and is safe to
// apply repeatedly (idempotent on already-clean input).

const PROMPT_PREFIX_RE = /^(?:\$ |> |❯ |# )/;
const FENCE_LINE_RE = /^```[^\n]*$/;

/** Normalize line endings to \n so transforms don't have to handle CRLF. */
function normalizeEol(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/** Replace newlines with spaces and collapse runs of whitespace. */
export function joinLines(text: string): string {
  const normalized = normalizeEol(text);
  return normalized.replace(/\s+/g, " ").trim();
}

/** Remove trailing backslash-newline continuations, merging to one line. */
export function unwrapContinuations(text: string): string {
  const normalized = normalizeEol(text);
  // Consume horizontal whitespace around the `\<LF>` split so the joined
  // result uses exactly one space between tokens, not two. Trailing
  // whitespace between the `\` and the newline is common in pasted text
  // (terminal padding, copy-from-editor artifacts) and should still count
  // as a continuation.
  return normalized.replace(/[ \t]*\\[ \t]*\n[ \t]*/g, " ");
}

/** Strip leading `$ `, `> `, `❯ `, `# ` from each line. */
export function stripPromptPrefix(text: string): string {
  return normalizeEol(text)
    .split("\n")
    .map((line) => line.replace(PROMPT_PREFIX_RE, ""))
    .join("\n");
}

/**
 * Strip markdown code fences: leading/trailing lines starting with ``` are
 * dropped. Useful for text copied out of Claude Code / Codex where the model
 * wrapped the command in a fenced block.
 */
export function stripCodeFence(text: string): string {
  const normalized = normalizeEol(text);
  const lines = normalized.split("\n");
  let start = 0;
  let end = lines.length;
  if (lines.length > 0 && FENCE_LINE_RE.test(lines[0])) start = 1;
  if (end > start && FENCE_LINE_RE.test(lines[end - 1])) end -= 1;
  return lines.slice(start, end).join("\n");
}

/** Replace curly single/double quotes with straight ASCII quotes. */
export function smartQuotesToStraight(text: string): string {
  return text
    .replace(/[“”„‟]/g, '"')
    .replace(/[‘’‚‛]/g, "'");
}

/** Trim leading and trailing whitespace from the whole document. */
export function trimDocument(text: string): string {
  return text.trim();
}
