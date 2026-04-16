import { describe, expect, it } from "vitest";

import { getTerminalTheme, normalizeTheme } from "$lib/themes";

describe("themes", () => {
  it("returns an app-owned terminal theme for normalized presets", () => {
    const theme = getTerminalTheme("deep-blue");

    expect(theme).toEqual({
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
    });
  });

  it("falls back to the default preset when theme input is invalid", () => {
    expect(normalizeTheme("not-a-theme")).toBe("deep-blue");
    expect(getTerminalTheme("not-a-theme")).toEqual(getTerminalTheme("deep-blue"));
  });
});
