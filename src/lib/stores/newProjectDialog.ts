import { writable, type Readable } from "svelte/store";
import type { Project } from "$lib/types";

/**
 * App-global "open the new-project dialog" pump. Mirrors customProfileModal:
 * commands can't mount the dialog themselves, so they flip this store and
 * App.svelte renders <NewProjectDialog> against its current state.
 *
 * `project: null` → create mode. `project: <existing>` → edit mode.
 */
interface NewProjectDialogState {
  visible: boolean;
  project: Project | null;
}

const state = writable<NewProjectDialogState>({ visible: false, project: null });

export const newProjectDialogState: Readable<NewProjectDialogState> = {
  subscribe: state.subscribe,
};

export function openNewProjectDialog(): void {
  state.set({ visible: true, project: null });
}

export function openEditProjectDialog(project: Project): void {
  state.set({ visible: true, project });
}

export function closeNewProjectDialog(): void {
  state.set({ visible: false, project: null });
}
