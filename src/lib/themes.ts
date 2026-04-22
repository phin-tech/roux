import type { ThemePreset } from "$lib/types";
import type { UserTerminalTheme } from "$lib/bindings";

export interface ThemeDefinition {
  id: ThemePreset;
  label: string;
  description: string;
  light?: boolean;
}

export interface TerminalAnsiPalette {
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  ansi: TerminalAnsiPalette;
}

export const THEME_DEFINITIONS: ThemeDefinition[] = [
  {
    id: "deep-blue",
    label: "Refined Zinc",
    description: "Neutral industrial grays with sky accents. The default.",
  },
  {
    id: "midnight-copper",
    label: "Midnight Copper",
    description: "Smoky navy workspace with copper highlights.",
  },
  {
    id: "steel-amber",
    label: "Steel Amber",
    description: "Warm chrome with amber highlights.",
  },
  {
    id: "slate-emerald",
    label: "Slate Emerald",
    description: "Dark slate with calmer green accents.",
  },
  {
    id: "graphite-rose",
    label: "Graphite Rose",
    description: "Graphite UI with rose and violet accents.",
  },
  {
    id: "nordic-night",
    label: "Nordic Night",
    description: "Cool blue-grays with teal accents. Inspired by Nord.",
  },
  {
    id: "cyber-audit",
    label: "Cyber Audit",
    description: "High contrast black with lime accents.",
  },
  {
    id: "mocha-soft",
    label: "Mocha Soft",
    description: "Warm dark palette inspired by Catppuccin.",
  },
  {
    id: "paper-ink",
    label: "Paper & Ink",
    description: "Clean light theme with a recessed dark terminal.",
    light: true,
  },
  {
    id: "github-day",
    label: "GitHub Day",
    description: "Light UI with a dark terminal workspace.",
    light: true,
  },
];

export const LIGHT_THEMES: Set<ThemePreset> = new Set(
  THEME_DEFINITIONS.filter((t) => t.light).map((t) => t.id),
);

const DEFAULT_THEME: ThemePreset = "deep-blue";

export function normalizeTheme(theme: string | null | undefined): ThemePreset {
  if (!theme || theme === "dark") {
    return DEFAULT_THEME;
  }

  return THEME_DEFINITIONS.some((definition) => definition.id === theme)
    ? (theme as ThemePreset)
    : DEFAULT_THEME;
}

export function isLightTheme(theme: ThemePreset): boolean {
  return LIGHT_THEMES.has(theme);
}

const TERMINAL_THEMES: Record<ThemePreset, TerminalTheme> = {
  "deep-blue": {
    background: "#09090b",
    foreground: "#fafafa",
    cursor: "#7dd3fc",
    selectionBackground: "#27272acc",
    ansi: {
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
  },
  "midnight-copper": {
    background: "#171d2a",
    foreground: "#d6deeb",
    cursor: "#f29d6f",
    selectionBackground: "#2e3b5688",
    ansi: {
      black: "#171d2a",
      red: "#ef8a77",
      green: "#8bcf9f",
      yellow: "#f2b37f",
      blue: "#6f95d8",
      magenta: "#b39ddb",
      cyan: "#7fb8c9",
      white: "#d6deeb",
      brightBlack: "#4a556d",
      brightRed: "#f6a596",
      brightGreen: "#a9dcb7",
      brightYellow: "#f7cfa9",
      brightBlue: "#90afeb",
      brightMagenta: "#c8b6e5",
      brightCyan: "#9fceda",
      brightWhite: "#e9effa",
    },
  },
  "steel-amber": {
    background: "#09090b",
    foreground: "#fafaf9",
    cursor: "#f59e0b",
    selectionBackground: "#27272acc",
    ansi: {
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
  },
  "slate-emerald": {
    background: "#09090b",
    foreground: "#e7f7f2",
    cursor: "#34d399",
    selectionBackground: "#15332ccc",
    ansi: {
      black: "#09090b",
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
  },
  "graphite-rose": {
    background: "#09090b",
    foreground: "#f5f2f8",
    cursor: "#fb7185",
    selectionBackground: "#2b1830cc",
    ansi: {
      black: "#09090b",
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
  },
  "nordic-night": {
    background: "#0c0e14",
    foreground: "#d8dee9",
    cursor: "#2dd4bf",
    selectionBackground: "#2e344088",
    ansi: {
      black: "#0c0e14",
      red: "#bf616a",
      green: "#a3be8c",
      yellow: "#ebcb8b",
      blue: "#81a1c1",
      magenta: "#b48ead",
      cyan: "#88c0d0",
      white: "#d8dee9",
      brightBlack: "#4c566a",
      brightRed: "#d08770",
      brightGreen: "#a3be8c",
      brightYellow: "#ebcb8b",
      brightBlue: "#5e81ac",
      brightMagenta: "#b48ead",
      brightCyan: "#8fbcbb",
      brightWhite: "#eceff4",
    },
  },
  "cyber-audit": {
    background: "#000000",
    foreground: "#ffffff",
    cursor: "#a3e635",
    selectionBackground: "#27272a88",
    ansi: {
      black: "#000000",
      red: "#e11d48",
      green: "#a3e635",
      yellow: "#facc15",
      blue: "#38bdf8",
      magenta: "#d946ef",
      cyan: "#22d3ee",
      white: "#ffffff",
      brightBlack: "#52525b",
      brightRed: "#fb7185",
      brightGreen: "#bef264",
      brightYellow: "#fde68a",
      brightBlue: "#7dd3fc",
      brightMagenta: "#e879f9",
      brightCyan: "#67e8f9",
      brightWhite: "#ffffff",
    },
  },
  "mocha-soft": {
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    cursor: "#89b4fa",
    selectionBackground: "#31324488",
    ansi: {
      black: "#45475a",
      red: "#f38ba8",
      green: "#a6e3a1",
      yellow: "#f9e2af",
      blue: "#89b4fa",
      magenta: "#cba6f7",
      cyan: "#94e2d5",
      white: "#bac2de",
      brightBlack: "#585b70",
      brightRed: "#f38ba8",
      brightGreen: "#a6e3a1",
      brightYellow: "#f9e2af",
      brightBlue: "#89b4fa",
      brightMagenta: "#cba6f7",
      brightCyan: "#94e2d5",
      brightWhite: "#a6adc8",
    },
  },
  // Light themes use a dark terminal
  "paper-ink": {
    background: "#1c1c1c",
    foreground: "#e4e4e4",
    cursor: "#2563eb",
    selectionBackground: "#3b3b3b88",
    ansi: {
      black: "#1c1c1c",
      red: "#dc2626",
      green: "#16a34a",
      yellow: "#ca8a04",
      blue: "#2563eb",
      magenta: "#9333ea",
      cyan: "#0891b2",
      white: "#e4e4e4",
      brightBlack: "#525252",
      brightRed: "#ef4444",
      brightGreen: "#22c55e",
      brightYellow: "#eab308",
      brightBlue: "#3b82f6",
      brightMagenta: "#a855f7",
      brightCyan: "#06b6d4",
      brightWhite: "#fafafa",
    },
  },
  "github-day": {
    background: "#0d1117",
    foreground: "#e6edf3",
    cursor: "#58a6ff",
    selectionBackground: "#26384888",
    ansi: {
      black: "#0d1117",
      red: "#ff7b72",
      green: "#7ee787",
      yellow: "#d29922",
      blue: "#58a6ff",
      magenta: "#bc8cff",
      cyan: "#39d353",
      white: "#e6edf3",
      brightBlack: "#484f58",
      brightRed: "#ffa198",
      brightGreen: "#56d364",
      brightYellow: "#e3b341",
      brightBlue: "#79c0ff",
      brightMagenta: "#d2a8ff",
      brightCyan: "#56d364",
      brightWhite: "#f0f6fc",
    },
  },
};

export function getTerminalTheme(theme: string | null | undefined): TerminalTheme {
  return TERMINAL_THEMES[normalizeTheme(theme)];
}

// ---------------------------------------------------------------------------
// Terminal-only themes (decoupled from GUI theme)
// ---------------------------------------------------------------------------
//
// The terminal can be driven by any of:
//   - "match-gui"  → follow the GUI theme's bundled palette (default, legacy
//                    behavior)
//   - one of the GUI theme IDs (e.g. "midnight-copper") → reuse that theme's
//                    bundled terminal palette without changing the GUI
//   - one of the editor-style IDs below (Dracula, Solarized, etc.) → a
//                    standalone palette with no GUI counterpart
//
// Adding a new editor palette: append to EDITOR_TERMINAL_THEMES *and* the
// editor section of TERMINAL_THEME_DEFINITIONS *and* the Rust
// `normalize_terminal_theme` allow-list. All three must agree.

export const MATCH_GUI_TERMINAL_THEME_ID = "match-gui";

export type TerminalThemeCategory = "auto" | "matching" | "editor" | "user";

export interface TerminalThemeDefinition {
  id: string;
  label: string;
  category: TerminalThemeCategory;
  description?: string;
}

// Popular iterm2colorschemes-inspired palettes. Picked for breadth of taste;
// not a comprehensive port of the upstream gallery.
const EDITOR_TERMINAL_THEMES: Record<string, TerminalTheme> = {
  dracula: {
    background: "#282a36",
    foreground: "#f8f8f2",
    cursor: "#f8f8f0",
    selectionBackground: "#44475a",
    ansi: {
      black: "#21222c",
      red: "#ff5555",
      green: "#50fa7b",
      yellow: "#f1fa8c",
      blue: "#bd93f9",
      magenta: "#ff79c6",
      cyan: "#8be9fd",
      white: "#f8f8f2",
      brightBlack: "#6272a4",
      brightRed: "#ff6e6e",
      brightGreen: "#69ff94",
      brightYellow: "#ffffa5",
      brightBlue: "#d6acff",
      brightMagenta: "#ff92df",
      brightCyan: "#a4ffff",
      brightWhite: "#ffffff",
    },
  },
  "solarized-dark": {
    background: "#002b36",
    foreground: "#839496",
    cursor: "#93a1a1",
    selectionBackground: "#073642",
    ansi: {
      black: "#073642",
      red: "#dc322f",
      green: "#859900",
      yellow: "#b58900",
      blue: "#268bd2",
      magenta: "#d33682",
      cyan: "#2aa198",
      white: "#eee8d5",
      brightBlack: "#002b36",
      brightRed: "#cb4b16",
      brightGreen: "#586e75",
      brightYellow: "#657b83",
      brightBlue: "#839496",
      brightMagenta: "#6c71c4",
      brightCyan: "#93a1a1",
      brightWhite: "#fdf6e3",
    },
  },
  "solarized-light": {
    background: "#fdf6e3",
    foreground: "#657b83",
    cursor: "#586e75",
    selectionBackground: "#eee8d5",
    ansi: {
      black: "#073642",
      red: "#dc322f",
      green: "#859900",
      yellow: "#b58900",
      blue: "#268bd2",
      magenta: "#d33682",
      cyan: "#2aa198",
      white: "#eee8d5",
      brightBlack: "#002b36",
      brightRed: "#cb4b16",
      brightGreen: "#586e75",
      brightYellow: "#657b83",
      brightBlue: "#839496",
      brightMagenta: "#6c71c4",
      brightCyan: "#93a1a1",
      brightWhite: "#fdf6e3",
    },
  },
  monokai: {
    background: "#272822",
    foreground: "#f8f8f2",
    cursor: "#f8f8f0",
    selectionBackground: "#49483e",
    ansi: {
      black: "#272822",
      red: "#f92672",
      green: "#a6e22e",
      yellow: "#f4bf75",
      blue: "#66d9ef",
      magenta: "#ae81ff",
      cyan: "#a1efe4",
      white: "#f8f8f2",
      brightBlack: "#75715e",
      brightRed: "#f92672",
      brightGreen: "#a6e22e",
      brightYellow: "#f4bf75",
      brightBlue: "#66d9ef",
      brightMagenta: "#ae81ff",
      brightCyan: "#a1efe4",
      brightWhite: "#f9f8f5",
    },
  },
  nord: {
    background: "#2e3440",
    foreground: "#d8dee9",
    cursor: "#d8dee9",
    selectionBackground: "#4c566a",
    ansi: {
      black: "#3b4252",
      red: "#bf616a",
      green: "#a3be8c",
      yellow: "#ebcb8b",
      blue: "#81a1c1",
      magenta: "#b48ead",
      cyan: "#88c0d0",
      white: "#e5e9f0",
      brightBlack: "#4c566a",
      brightRed: "#bf616a",
      brightGreen: "#a3be8c",
      brightYellow: "#ebcb8b",
      brightBlue: "#81a1c1",
      brightMagenta: "#b48ead",
      brightCyan: "#8fbcbb",
      brightWhite: "#eceff4",
    },
  },
  "gruvbox-dark": {
    background: "#282828",
    foreground: "#ebdbb2",
    cursor: "#ebdbb2",
    selectionBackground: "#3c3836",
    ansi: {
      black: "#282828",
      red: "#cc241d",
      green: "#98971a",
      yellow: "#d79921",
      blue: "#458588",
      magenta: "#b16286",
      cyan: "#689d6a",
      white: "#a89984",
      brightBlack: "#928374",
      brightRed: "#fb4934",
      brightGreen: "#b8bb26",
      brightYellow: "#fabd2f",
      brightBlue: "#83a598",
      brightMagenta: "#d3869b",
      brightCyan: "#8ec07c",
      brightWhite: "#ebdbb2",
    },
  },
  "tokyo-night": {
    background: "#1a1b26",
    foreground: "#c0caf5",
    cursor: "#c0caf5",
    selectionBackground: "#283457",
    ansi: {
      black: "#15161e",
      red: "#f7768e",
      green: "#9ece6a",
      yellow: "#e0af68",
      blue: "#7aa2f7",
      magenta: "#bb9af7",
      cyan: "#7dcfff",
      white: "#a9b1d6",
      brightBlack: "#414868",
      brightRed: "#f7768e",
      brightGreen: "#9ece6a",
      brightYellow: "#e0af68",
      brightBlue: "#7aa2f7",
      brightMagenta: "#bb9af7",
      brightCyan: "#7dcfff",
      brightWhite: "#c0caf5",
    },
  },
  "one-dark": {
    background: "#282c34",
    foreground: "#abb2bf",
    cursor: "#528bff",
    selectionBackground: "#3e4451",
    ansi: {
      black: "#282c34",
      red: "#e06c75",
      green: "#98c379",
      yellow: "#d19a66",
      blue: "#61afef",
      magenta: "#c678dd",
      cyan: "#56b6c2",
      white: "#abb2bf",
      brightBlack: "#5c6370",
      brightRed: "#e06c75",
      brightGreen: "#98c379",
      brightYellow: "#d19a66",
      brightBlue: "#61afef",
      brightMagenta: "#c678dd",
      brightCyan: "#56b6c2",
      brightWhite: "#ffffff",
    },
  },
  "catppuccin-mocha": {
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    cursor: "#f5e0dc",
    selectionBackground: "#585b70",
    ansi: {
      black: "#45475a",
      red: "#f38ba8",
      green: "#a6e3a1",
      yellow: "#f9e2af",
      blue: "#89b4fa",
      magenta: "#f5c2e7",
      cyan: "#94e2d5",
      white: "#bac2de",
      brightBlack: "#585b70",
      brightRed: "#f38ba8",
      brightGreen: "#a6e3a1",
      brightYellow: "#f9e2af",
      brightBlue: "#89b4fa",
      brightMagenta: "#f5c2e7",
      brightCyan: "#94e2d5",
      brightWhite: "#a6adc8",
    },
  },
  "github-dark": {
    background: "#0d1117",
    foreground: "#c9d1d9",
    cursor: "#58a6ff",
    selectionBackground: "#163356",
    ansi: {
      black: "#484f58",
      red: "#ff7b72",
      green: "#3fb950",
      yellow: "#d29922",
      blue: "#58a6ff",
      magenta: "#bc8cff",
      cyan: "#39c5cf",
      white: "#b1bac4",
      brightBlack: "#6e7681",
      brightRed: "#ffa198",
      brightGreen: "#56d364",
      brightYellow: "#e3b341",
      brightBlue: "#79c0ff",
      brightMagenta: "#d2a8ff",
      brightCyan: "#56d4dd",
      brightWhite: "#f0f6fc",
    },
  },
};

export const TERMINAL_THEME_DEFINITIONS: TerminalThemeDefinition[] = [
  {
    id: MATCH_GUI_TERMINAL_THEME_ID,
    label: "Match GUI Theme",
    category: "auto",
    description: "Follow the terminal palette bundled with the active GUI theme.",
  },
  ...THEME_DEFINITIONS.map<TerminalThemeDefinition>((t) => ({
    id: t.id,
    label: t.label,
    category: "matching",
    description: `Terminal palette from the ${t.label} GUI theme.`,
  })),
  { id: "dracula", label: "Dracula", category: "editor" },
  { id: "solarized-dark", label: "Solarized Dark", category: "editor" },
  { id: "solarized-light", label: "Solarized Light", category: "editor" },
  { id: "monokai", label: "Monokai", category: "editor" },
  { id: "nord", label: "Nord", category: "editor" },
  { id: "gruvbox-dark", label: "Gruvbox Dark", category: "editor" },
  { id: "tokyo-night", label: "Tokyo Night", category: "editor" },
  { id: "one-dark", label: "One Dark", category: "editor" },
  { id: "catppuccin-mocha", label: "Catppuccin Mocha", category: "editor" },
  { id: "github-dark", label: "GitHub Dark", category: "editor" },
];

const BUILTIN_TERMINAL_THEME_IDS = new Set(TERMINAL_THEME_DEFINITIONS.map((d) => d.id));

// `user:*` IDs are accepted unconditionally — the file may be missing at
// validation time but will resolve once the user themes are reloaded.
function isAcceptableTerminalThemeId(id: string): boolean {
  if (BUILTIN_TERMINAL_THEME_IDS.has(id)) return true;
  return id.startsWith("user:") && id.length > "user:".length;
}

export function normalizeTerminalThemeId(id: string | null | undefined): string {
  if (!id) return MATCH_GUI_TERMINAL_THEME_ID;
  return isAcceptableTerminalThemeId(id) ? id : MATCH_GUI_TERMINAL_THEME_ID;
}

/**
 * Merge built-in terminal theme definitions with the user-supplied ones
 * loaded from `~/.config/roux/themes/`. The "User" group is appended last
 * and only when non-empty; this keeps the picker tidy for users who
 * haven't dropped any files in the folder.
 */
export function getAllTerminalThemeDefinitions(
  userThemes: ReadonlyArray<UserTerminalTheme>,
): TerminalThemeDefinition[] {
  if (userThemes.length === 0) {
    return TERMINAL_THEME_DEFINITIONS;
  }
  const userDefs = userThemes.map<TerminalThemeDefinition>((t) => ({
    id: t.id,
    label: t.label,
    category: "user",
    description: "From ~/.config/roux/themes/",
  }));
  return [...TERMINAL_THEME_DEFINITIONS, ...userDefs];
}

export function resolveTerminalTheme(
  uiTheme: string | null | undefined,
  terminalThemeId: string | null | undefined,
  userThemes: ReadonlyArray<UserTerminalTheme> = [],
): TerminalTheme {
  const id = normalizeTerminalThemeId(terminalThemeId);
  if (id === MATCH_GUI_TERMINAL_THEME_ID) {
    return TERMINAL_THEMES[normalizeTheme(uiTheme)];
  }
  if (id.startsWith("user:")) {
    const user = userThemes.find((t) => t.id === id);
    if (user) return user.palette;
    // File missing — fall through to GUI palette so the terminal still
    // renders something sane until the user fixes/reloads.
    return TERMINAL_THEMES[normalizeTheme(uiTheme)];
  }
  const matching = TERMINAL_THEMES[id as ThemePreset];
  if (matching) return matching;
  const editor = EDITOR_TERMINAL_THEMES[id];
  if (editor) return editor;
  return TERMINAL_THEMES[normalizeTheme(uiTheme)];
}
