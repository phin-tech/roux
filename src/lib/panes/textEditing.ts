export interface TextEditState {
  value: string;
  selectionStart: number;
  selectionEnd: number;
}

export interface CopyTextEditState extends TextEditState {
  clipboardText: string;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function normalizeState(state: TextEditState): TextEditState {
  const length = state.value.length;
  const start = clamp(state.selectionStart, 0, length);
  const end = clamp(state.selectionEnd, 0, length);
  return {
    value: state.value,
    selectionStart: Math.min(start, end),
    selectionEnd: Math.max(start, end),
  };
}

function replaceRange(
  state: TextEditState,
  start: number,
  end: number,
  replacement: string,
): TextEditState {
  const nextValue =
    state.value.slice(0, start) + replacement + state.value.slice(end);
  const nextCursor = start + replacement.length;
  return {
    value: nextValue,
    selectionStart: nextCursor,
    selectionEnd: nextCursor,
  };
}

function lineRangeAt(
  value: string,
  index: number,
): { start: number; end: number } {
  const clampedIndex = clamp(index, 0, value.length);
  const start = value.lastIndexOf("\n", Math.max(0, clampedIndex - 1)) + 1;
  const nextNewline = value.indexOf("\n", clampedIndex);
  return {
    start,
    end: nextNewline === -1 ? value.length : nextNewline,
  };
}

function hasSelection(state: TextEditState): boolean {
  return state.selectionStart !== state.selectionEnd;
}

function isWordChar(char: string): boolean {
  return /[A-Za-z0-9_]/.test(char);
}

function isWhitespace(char: string): boolean {
  return /\s/.test(char);
}

export function insertAtSelection(
  state: TextEditState,
  text: string,
): TextEditState {
  const normalized = normalizeState(state);
  return replaceRange(
    normalized,
    normalized.selectionStart,
    normalized.selectionEnd,
    text,
  );
}

export function clearBuffer(_state: TextEditState): TextEditState {
  return {
    value: "",
    selectionStart: 0,
    selectionEnd: 0,
  };
}

export function copyAndClearCurrentLine(
  state: TextEditState,
): CopyTextEditState {
  const normalized = normalizeState(state);
  const range = lineRangeAt(normalized.value, normalized.selectionStart);
  const next = replaceRange(normalized, range.start, range.end, "");
  return {
    ...next,
    clipboardText: normalized.value.slice(range.start, range.end),
  };
}

export function clearSelectedLines(state: TextEditState): TextEditState {
  const normalized = normalizeState(state);
  const startRange = lineRangeAt(normalized.value, normalized.selectionStart);
  const endIndex = hasSelection(normalized)
    ? Math.max(normalized.selectionStart, normalized.selectionEnd - 1)
    : normalized.selectionEnd;
  const endRange = lineRangeAt(normalized.value, endIndex);
  const clearedText = normalized.value.slice(startRange.start, endRange.end);
  const replacement = "\n".repeat(clearedText.split("\n").length - 1);
  const next = replaceRange(
    normalized,
    startRange.start,
    endRange.end,
    replacement,
  );
  return {
    ...next,
    selectionStart: startRange.start,
    selectionEnd: startRange.start,
  };
}

export function deleteWordLeft(state: TextEditState): TextEditState {
  const normalized = normalizeState(state);
  if (hasSelection(normalized)) {
    return replaceRange(
      normalized,
      normalized.selectionStart,
      normalized.selectionEnd,
      "",
    );
  }

  let start = normalized.selectionStart;
  while (start > 0 && isWhitespace(normalized.value[start - 1])) start -= 1;
  if (start > 0 && isWordChar(normalized.value[start - 1])) {
    while (start > 0 && isWordChar(normalized.value[start - 1])) start -= 1;
  } else {
    while (
      start > 0 &&
      !isWhitespace(normalized.value[start - 1]) &&
      !isWordChar(normalized.value[start - 1])
    ) {
      start -= 1;
    }
  }

  return replaceRange(normalized, start, normalized.selectionStart, "");
}

export function deleteToLineStart(state: TextEditState): TextEditState {
  const normalized = normalizeState(state);
  if (hasSelection(normalized)) {
    return replaceRange(
      normalized,
      normalized.selectionStart,
      normalized.selectionEnd,
      "",
    );
  }
  const range = lineRangeAt(normalized.value, normalized.selectionStart);
  return replaceRange(normalized, range.start, normalized.selectionStart, "");
}

export function deleteToLineEnd(state: TextEditState): TextEditState {
  const normalized = normalizeState(state);
  if (hasSelection(normalized)) {
    return replaceRange(
      normalized,
      normalized.selectionStart,
      normalized.selectionEnd,
      "",
    );
  }
  const range = lineRangeAt(normalized.value, normalized.selectionStart);
  return replaceRange(normalized, normalized.selectionStart, range.end, "");
}
