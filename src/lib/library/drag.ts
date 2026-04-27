import { writable } from "svelte/store";
import type { LibraryItem } from "$lib/tauri";

export const LIBRARY_PROMPT_DRAG_MIME = "application/x-roux-library-prompt";

export interface LibraryPromptDragPayload {
  itemId: string;
  title: string;
}

export const draggedLibraryPrompt = writable<LibraryPromptDragPayload | null>(null);

export function libraryPromptDragPayload(item: LibraryItem): LibraryPromptDragPayload | null {
  if (item.itemType !== "prompt") return null;
  return {
    itemId: item.id,
    title: item.title,
  };
}

export function writeLibraryPromptDragData(dataTransfer: DataTransfer, item: LibraryItem): boolean {
  const payload = libraryPromptDragPayload(item);
  if (!payload) return false;

  draggedLibraryPrompt.set(payload);
  dataTransfer.effectAllowed = "copy";
  dataTransfer.setData(LIBRARY_PROMPT_DRAG_MIME, JSON.stringify(payload));
  dataTransfer.setData("text/plain", item.title);
  return true;
}

export function clearDraggedLibraryPrompt(): void {
  draggedLibraryPrompt.set(null);
}

export function hasLibraryPromptDragData(dataTransfer: DataTransfer | null): boolean {
  if (!dataTransfer) return false;
  return Array.from(dataTransfer.types).includes(LIBRARY_PROMPT_DRAG_MIME);
}

export function readLibraryPromptDragData(dataTransfer: DataTransfer | null): LibraryPromptDragPayload | null {
  if (!dataTransfer) return null;

  const raw = dataTransfer.getData(LIBRARY_PROMPT_DRAG_MIME);
  if (!raw) return null;

  try {
    const parsed = JSON.parse(raw) as Partial<LibraryPromptDragPayload>;
    if (typeof parsed.itemId !== "string" || parsed.itemId.trim() === "") return null;
    if (typeof parsed.title !== "string") return null;
    return {
      itemId: parsed.itemId,
      title: parsed.title,
    };
  } catch {
    return null;
  }
}
