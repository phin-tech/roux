import { writable, get } from "svelte/store";

export type Side = "left" | "right";

export interface SidebarLayout {
  width: number;
  splitRatio: number;
  railSide: Side;
  hidden: boolean;
}

const STORAGE_KEY = "roux.sidebar.dock";
export const MIN_DOCK_WIDTH = 240;
export const MAX_DOCK_WIDTH = 800;
const DEFAULT_WIDTH = 320;
const DEFAULT_SPLIT = 0.5;
const DEFAULT_RAIL_SIDE: Side = "right";

function clampWidth(w: number): number {
  if (!Number.isFinite(w)) return DEFAULT_WIDTH;
  return Math.max(MIN_DOCK_WIDTH, Math.min(MAX_DOCK_WIDTH, w));
}

function clampRatio(r: number): number {
  if (!Number.isFinite(r)) return DEFAULT_SPLIT;
  return Math.max(0.2, Math.min(0.8, r));
}

function isFiniteNumber(v: unknown): v is number {
  return typeof v === "number" && Number.isFinite(v);
}

function normalizeSide(s: unknown): Side | null {
  return s === "left" ? "left" : s === "right" ? "right" : null;
}

function loadInitial(): SidebarLayout {
  const defaults: SidebarLayout = {
    width: DEFAULT_WIDTH,
    splitRatio: DEFAULT_SPLIT,
    railSide: DEFAULT_RAIL_SIDE,
    hidden: false,
  };
  try {
    if (typeof window === "undefined" || !window.localStorage) return defaults;
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const railSide =
      normalizeSide(parsed.railSide) ??
      normalizeSide(parsed.dockSide) ??
      DEFAULT_RAIL_SIDE;
    return {
      width: clampWidth(isFiniteNumber(parsed.width) ? parsed.width : DEFAULT_WIDTH),
      splitRatio: clampRatio(
        isFiniteNumber(parsed.splitRatio) ? parsed.splitRatio : DEFAULT_SPLIT,
      ),
      railSide,
      hidden: typeof parsed.hidden === "boolean" ? parsed.hidden : false,
    };
  } catch {
    return defaults;
  }
}

export const sidebarLayout = writable<SidebarLayout>(loadInitial());

sidebarLayout.subscribe((v) => {
  try {
    if (typeof window === "undefined" || !window.localStorage) return;
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(v));
  } catch {}
});

export function setDockWidth(w: number): void {
  sidebarLayout.update((s) => ({ ...s, width: clampWidth(w) }));
}

export function setDockSplit(r: number): void {
  sidebarLayout.update((s) => ({ ...s, splitRatio: clampRatio(r) }));
}

export function setRailSide(side: Side): void {
  sidebarLayout.update((s) => ({ ...s, railSide: side }));
}

export function toggleRailSide(): void {
  sidebarLayout.update((s) => ({
    ...s,
    railSide: s.railSide === "left" ? "right" : "left",
  }));
}

export function showSidebar(): void {
  sidebarLayout.update((s) => ({ ...s, hidden: false }));
}

export function hideSidebar(): void {
  sidebarLayout.update((s) => ({ ...s, hidden: true }));
}

export function toggleSidebarHidden(): void {
  sidebarLayout.update((s) => ({ ...s, hidden: !s.hidden }));
}

export function snapshotSidebarLayout(): SidebarLayout {
  return get(sidebarLayout);
}
