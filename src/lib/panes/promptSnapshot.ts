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

// Matches `$`, `>`, `❯`, `#` followed by whitespace OR end-of-string, so
// a bare prompt marker on an otherwise empty line also gets stripped.
const PROMPT_PREFIX_RE = /^(?:\$|>|❯|#)(?:\s+|$)/;

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
export function readPromptSnapshot(buffer: SnapshotBuffer): PromptSnapshot | null {
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

  const pieces: string[] = [];
  for (let y = startLine; y <= absCursor; y++) {
    const line = buffer.getLine(y);
    if (!line) continue;
    pieces.push(line.translateToString(true));
  }
  const joined = pieces.join("");
  const stripped = joined.replace(/^\s+/, "").replace(PROMPT_PREFIX_RE, "");
  const text = stripped.trimEnd();

  if (!text) return null;
  return { text, seeded: true };
}
