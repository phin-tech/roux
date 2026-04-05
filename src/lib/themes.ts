import type { ITheme } from "@xterm/xterm";
import type { ThemePreset } from "$lib/types";

export interface ThemeDefinition {
  id: ThemePreset;
  label: string;
  description: string;
}

export const THEME_DEFINITIONS: ThemeDefinition[] = [
  {
    id: "deep-blue",
    label: "Deep Blue",
    description: "Cool slate and sky accents. Closest to the current look.",
  },
  {
    id: "steel-amber",
    label: "Steel Amber",
    description: "Neutral chrome with warm highlights and a less blue bias.",
  },
  {
    id: "slate-emerald",
    label: "Slate Emerald",
    description: "Dark slate surfaces with calmer green accents.",
  },
  {
    id: "graphite-rose",
    label: "Graphite Rose",
    description: "Graphite UI with rose accents for a less conventional feel.",
  },
];

const DEFAULT_THEME: ThemePreset = "deep-blue";

export function normalizeTheme(theme: string | null | undefined): ThemePreset {
  if (!theme || theme === "dark") {
    return DEFAULT_THEME;
  }

  return THEME_DEFINITIONS.some((definition) => definition.id === theme)
    ? theme as ThemePreset
    : DEFAULT_THEME;
}

const XTERM_THEMES: Record<ThemePreset, ITheme> = {
  "deep-blue": {
    background: "#09090b",
    foreground: "#fafafa",
    cursor: "#7dd3fc",
    selectionBackground: "#27272acc",
    black: "#09090b",
    red: "#fb7185",
    green: "#4ade80",
    yellow: "#fbbf24",
    blue: "#38bdf8",
    magenta: "#c084fc",
    cyan: "#67e8f9",
    white: "#fafafa",
    brightBlack: "#52525b",
    brightRed: "#fda4af",
    brightGreen: "#86efac",
    brightYellow: "#fde68a",
    brightBlue: "#7dd3fc",
    brightMagenta: "#d8b4fe",
    brightCyan: "#a5f3fc",
    brightWhite: "#fafafa",
  },
  "steel-amber": {
    background: "#09090b",
    foreground: "#fafaf9",
    cursor: "#f59e0b",
    selectionBackground: "#27272acc",
    black: "#09090b",
    red: "#fb7185",
    green: "#86efac",
    yellow: "#f59e0b",
    blue: "#94a3b8",
    magenta: "#c084fc",
    cyan: "#cbd5e1",
    white: "#f1f5f9",
    brightBlack: "#52525b",
    brightRed: "#fda4af",
    brightGreen: "#bbf7d0",
    brightYellow: "#fcd34d",
    brightBlue: "#cbd5e1",
    brightMagenta: "#e9d5ff",
    brightCyan: "#e2e8f0",
    brightWhite: "#fafaf9",
  },
  "slate-emerald": {
    background: "#07110f",
    foreground: "#e7f7f2",
    cursor: "#34d399",
    selectionBackground: "#15332ccc",
    black: "#07110f",
    red: "#fb7185",
    green: "#34d399",
    yellow: "#fbbf24",
    blue: "#60a5fa",
    magenta: "#a78bfa",
    cyan: "#5eead4",
    white: "#e7f7f2",
    brightBlack: "#4b635c",
    brightRed: "#fda4af",
    brightGreen: "#6ee7b7",
    brightYellow: "#fde68a",
    brightBlue: "#93c5fd",
    brightMagenta: "#c4b5fd",
    brightCyan: "#99f6e4",
    brightWhite: "#f0fdfa",
  },
  "graphite-rose": {
    background: "#0b0a0f",
    foreground: "#f5f2f8",
    cursor: "#fb7185",
    selectionBackground: "#2b1830cc",
    black: "#0b0a0f",
    red: "#fb7185",
    green: "#4ade80",
    yellow: "#fbbf24",
    blue: "#a78bfa",
    magenta: "#f472b6",
    cyan: "#94a3b8",
    white: "#f5f2f8",
    brightBlack: "#5b5464",
    brightRed: "#fda4af",
    brightGreen: "#86efac",
    brightYellow: "#fde68a",
    brightBlue: "#c4b5fd",
    brightMagenta: "#f9a8d4",
    brightCyan: "#cbd5e1",
    brightWhite: "#faf5ff",
  },
};

export function getXtermTheme(theme: string | null | undefined): ITheme {
  return XTERM_THEMES[normalizeTheme(theme)];
}
