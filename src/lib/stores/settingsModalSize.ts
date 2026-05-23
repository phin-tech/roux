import { writable } from "svelte/store";

export interface SettingsModalSize {
  width: number;
  height: number;
}

const STORAGE_KEY = "roux.settings.modalSize";

// Absolute clamp bounds. The component additionally clamps against the live
// viewport, but these guard against absurd persisted values and runaway drags.
export const SETTINGS_MODAL_MIN_WIDTH = 560;
export const SETTINGS_MODAL_MAX_WIDTH = 1400;
export const SETTINGS_MODAL_MIN_HEIGHT = 400;
export const SETTINGS_MODAL_MAX_HEIGHT = 1000;

const DEFAULT_WIDTH = 860;
const DEFAULT_HEIGHT = 640;

function clampWidth(w: number): number {
  if (!Number.isFinite(w)) return DEFAULT_WIDTH;
  return Math.max(SETTINGS_MODAL_MIN_WIDTH, Math.min(SETTINGS_MODAL_MAX_WIDTH, w));
}

function clampHeight(h: number): number {
  if (!Number.isFinite(h)) return DEFAULT_HEIGHT;
  return Math.max(SETTINGS_MODAL_MIN_HEIGHT, Math.min(SETTINGS_MODAL_MAX_HEIGHT, h));
}

function isFiniteNumber(v: unknown): v is number {
  return typeof v === "number" && Number.isFinite(v);
}

function loadInitial(): SettingsModalSize {
  const defaults: SettingsModalSize = {
    width: DEFAULT_WIDTH,
    height: DEFAULT_HEIGHT,
  };
  try {
    if (typeof window === "undefined" || !window.localStorage) return defaults;
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    return {
      width: clampWidth(isFiniteNumber(parsed.width) ? parsed.width : DEFAULT_WIDTH),
      height: clampHeight(isFiniteNumber(parsed.height) ? parsed.height : DEFAULT_HEIGHT),
    };
  } catch {
    return defaults;
  }
}

export const settingsModalSize = writable<SettingsModalSize>(loadInitial());

settingsModalSize.subscribe((v) => {
  try {
    if (typeof window === "undefined" || !window.localStorage) return;
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(v));
  } catch {}
});

export function setSettingsModalSize(width: number, height: number): void {
  settingsModalSize.set({ width: clampWidth(width), height: clampHeight(height) });
}
