import { get, writable } from "svelte/store";

export type MultiLineTarget = "shell" | "claude";

export interface MultiLineEditorState {
  open: boolean;
  paneId: string | null;
  paneLabel: string | null;
  initialText: string;
  seeded: boolean;
  target: MultiLineTarget;
}

const INITIAL_STATE: MultiLineEditorState = {
  open: false,
  paneId: null,
  paneLabel: null,
  initialText: "",
  seeded: false,
  target: "shell",
};

export const multiLineEditor = writable<MultiLineEditorState>(INITIAL_STATE);

export interface OpenMultiLineEditorOpts {
  paneId: string;
  paneLabel: string | null;
  initialText: string;
  seeded: boolean;
  target: MultiLineTarget;
}

export function openMultiLineEditor(opts: OpenMultiLineEditorOpts): void {
  multiLineEditor.set({
    open: true,
    paneId: opts.paneId,
    paneLabel: opts.paneLabel,
    initialText: opts.initialText,
    seeded: opts.seeded,
    target: opts.target,
  });
}

export function closeMultiLineEditor(): void {
  multiLineEditor.set(INITIAL_STATE);
}

export function isMultiLineEditorOpen(): boolean {
  return get(multiLineEditor).open;
}

/**
 * Build the exact byte sequence written to the PTY on submit.
 *
 *   - Shell panes: `Ctrl+E` + `Ctrl+U` clears the current readline buffer
 *     regardless of cursor position (Ctrl+U alone only kills cursor→BOL,
 *     leaving trailing chars when the cursor is mid-line).
 *   - All panes: content wrapped in bracketed-paste markers keeps multi-line
 *     text atomic — the shell treats it as one paste instead of a sequence
 *     of Enter-terminated lines.
 *   - Enter is **never** appended: the user reviews in the real terminal
 *     and submits themself.
 */
export function buildSubmitPayload(text: string, target: MultiLineTarget): string {
  const clear = target === "shell" ? "\x05\x15" : "";
  return `${clear}\x1b[200~${text}\x1b[201~`;
}
