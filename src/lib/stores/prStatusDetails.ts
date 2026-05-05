import { writable } from "svelte/store";

export const prStatusDetailsOpen = writable(false);

export function togglePrStatusDetails(): void {
  prStatusDetailsOpen.update((open) => !open);
}

export function closePrStatusDetails(): void {
  prStatusDetailsOpen.set(false);
}
