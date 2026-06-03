export interface PromptSnapshot {
  text: string;
  seeded: boolean;
}

/**
 * The subset of xterm.js's `IBuffer` we actually read. Typed structurally
 * so tests can pass a minimal fake without stubbing the full IBuffer API
 * (getNullCell, length, viewportY, ...). xterm's real IBuffer satisfies
 * this shape.
 */
export interface SnapshotBufferLine {
  isWrapped: boolean;
  translateToString(trimRight?: boolean): string;
}

export interface SnapshotBuffer {
  cursorY: number;
  baseY: number;
  getLine(y: number): SnapshotBufferLine | undefined;
}

// Matches `$`, `>`, `❯`, `#` followed by EXACTLY ONE whitespace char or
// end-of-string. One whitespace only — not `\s+` — so that if the user typed
// an extra leading space (common with HISTCONTROL=ignorespace), it survives
// the strip. `$ ` and `$  echo` both still get the marker stripped, but the
// latter keeps the user's leading space.
const PROMPT_PREFIX_RE = /^(?:\$|>|❯|#)(?:\s|$)/;

/**
 * Read the current logical prompt line from an xterm buffer. Walks backwards
 * through wrapped continuation lines so a long command typed across multiple
 * visual rows comes back as one string. Strips common prompt prefixes (`$ `,
 * `> `, `❯ `, `# `) from the origin line.
 *
 * Uses absolute buffer indices (`baseY + cursorY`) so the lookup is correct
 * when the terminal has been scrolled. Returns null if the line can't be
 * read or is empty after stripping.
 */
export function readPromptSnapshot(
  buffer: SnapshotBuffer,
): PromptSnapshot | null {
  const absCursor = buffer.baseY + buffer.cursorY;
  const cursorLine = buffer.getLine(absCursor);
  if (!cursorLine) return null;

  // Walk backward to find the origin of a wrapped block. A line's
  // `isWrapped` flag is true when it is the *continuation* of the previous
  // line, so we keep stepping back while the current line is a continuation.
  let startLine = absCursor;
  while (startLine > 0) {
    const line = buffer.getLine(startLine);
    if (!line?.isWrapped) break;
    startLine -= 1;
  }

  // Walk forward too — when the cursor is positioned mid-way through a
  // wrapped input (arrow-keyed back into the middle of a long command),
  // the rest of the logical line lives below the cursor row.
  let endLine = absCursor;
  while (true) {
    const next = buffer.getLine(endLine + 1);
    if (!next?.isWrapped) break;
    endLine += 1;
  }

  const pieces: string[] = [];
  for (let y = startLine; y <= endLine; y++) {
    const line = buffer.getLine(y);
    if (!line) continue;
    pieces.push(line.translateToString(true));
  }
  const joined = pieces.join("");
  const stripped = joined.replace(PROMPT_PREFIX_RE, "");
  const text = stripped.trimEnd();

  if (!text) return null;
  return { text, seeded: true };
}
