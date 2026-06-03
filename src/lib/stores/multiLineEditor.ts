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

export const MULTI_LINE_EDITOR_FOCUS_EVENT = "roux:focus-multiline-editor";

export interface MultiLineEditorFocusDetail {
  paneId: string;
}

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

export function requestMultiLineEditorFocus(paneId: string): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<MultiLineEditorFocusDetail>(MULTI_LINE_EDITOR_FOCUS_EVENT, {
      detail: { paneId },
    }),
  );
}

/**
 * Build the paste byte sequence written to the PTY on submit.
 *
 *   - Shell panes first send Ctrl+E + Ctrl+U to clear the active line editor
 *     buffer, then bracketed paste. Ctrl+U alone only clears from cursor to
 *     BOL, so Ctrl+E makes the clear independent of cursor position.
 *   - Claude panes use bracketed paste without shell line-clear bytes that
 *     would be interpreted by the TUI prompt itself.
 *
 * The submit Enter is intentionally written as a separate PTY write by the
 * component so shells handle it like a real keypress after paste completes.
 */
export function buildSubmitPayload(
  text: string,
  target: MultiLineTarget,
): string {
  const clear = target === "shell" ? "\x05\x15" : "";
  const normalizedText = text.replace(/[\r\n]+$/g, "");
  return `${clear}\x1b[200~${normalizedText}\x1b[201~`;
}
