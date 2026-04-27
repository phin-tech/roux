import { writable } from "svelte/store";
import type { LibraryItemType } from "$lib/tauri";

export interface LibraryWindowState {
  open: boolean;
  itemId: string | null;
  itemType: LibraryItemType;
  mode: "browse" | "new" | "edit";
}

const INITIAL_STATE: LibraryWindowState = {
  open: false,
  itemId: null,
  itemType: "prompt",
  mode: "browse",
};

export const libraryWindow = writable<LibraryWindowState>(INITIAL_STATE);

export function openLibraryWindow(): void {
  libraryWindow.set({ ...INITIAL_STATE, open: true });
}

export function openLibraryNew(itemType: LibraryItemType): void {
  libraryWindow.set({ open: true, itemId: null, itemType, mode: "new" });
}

export function openLibraryEdit(itemId: string, itemType: LibraryItemType): void {
  libraryWindow.set({ open: true, itemId, itemType, mode: "edit" });
}

export function closeLibraryWindow(): void {
  libraryWindow.set(INITIAL_STATE);
}
