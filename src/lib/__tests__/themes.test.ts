import { describe, expect, it } from "vitest";

import {
  getAllTerminalThemeDefinitions,
  getTerminalTheme,
  normalizeTerminalThemeId,
  normalizeTheme,
  resolveTerminalTheme,
} from "$lib/themes";
import type { UserTerminalTheme } from "$lib/bindings";

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

  it("recognizes the warm-burnout presets and surfaces their palettes", () => {
    expect(normalizeTheme("warm-burnout-dark")).toBe("warm-burnout-dark");
    expect(normalizeTheme("warm-burnout-light")).toBe("warm-burnout-light");

    const dark = getTerminalTheme("warm-burnout-dark");
    expect(dark.background).toBe("#1a1510");
    expect(dark.foreground).toBe("#bfbdb6");

    const light = getTerminalTheme("warm-burnout-light");
    expect(light.background).toBe("#f5ede0");
    expect(light.foreground).toBe("#3a3630");
  });

  describe("resolveTerminalTheme", () => {
    it("'match-gui' follows the active GUI theme", () => {
      expect(resolveTerminalTheme("midnight-copper", "match-gui")).toEqual(
        getTerminalTheme("midnight-copper"),
      );
      // null/undefined terminalTheme is treated as match-gui (legacy users).
      expect(resolveTerminalTheme("midnight-copper", null)).toEqual(
        getTerminalTheme("midnight-copper"),
      );
    });

    it("returns the editor palette when an editor ID is selected", () => {
      const dracula = resolveTerminalTheme("deep-blue", "dracula");
      expect(dracula.background).toBe("#282a36");
      expect(dracula.ansi.green).toBe("#50fa7b");
    });

    it("returns a GUI-matching palette decoupled from the active GUI theme", () => {
      // Pinning the terminal to nordic-night while the GUI is deep-blue should
      // surface the nordic palette, not the deep-blue one.
      expect(resolveTerminalTheme("deep-blue", "nordic-night")).toEqual(
        getTerminalTheme("nordic-night"),
      );
    });

    it("falls back to the GUI theme when the terminal ID is unknown", () => {
      expect(resolveTerminalTheme("deep-blue", "not-a-theme")).toEqual(
        getTerminalTheme("deep-blue"),
      );
      expect(normalizeTerminalThemeId("not-a-theme")).toBe("match-gui");
    });

    describe("user themes", () => {
      const userTheme: UserTerminalTheme = {
        id: "user:my-fav",
        label: "My Fav",
        palette: {
          background: "#101010",
          foreground: "#eeeeee",
          cursor: "#ff00ff",
          selectionBackground: "#202020",
          ansi: {
            black: "#000000",
            red: "#ff0000",
            green: "#00ff00",
            yellow: "#ffff00",
            blue: "#0000ff",
            magenta: "#ff00ff",
            cyan: "#00ffff",
            white: "#ffffff",
            brightBlack: "#404040",
            brightRed: "#ff4040",
            brightGreen: "#40ff40",
            brightYellow: "#ffff40",
            brightBlue: "#4040ff",
            brightMagenta: "#ff40ff",
            brightCyan: "#40ffff",
            brightWhite: "#f0f0f0",
          },
        },
      };

      it("normalizeTerminalThemeId accepts any user:* id", () => {
        expect(normalizeTerminalThemeId("user:my-fav")).toBe("user:my-fav");
        // Empty stem isn't a real id — fall back.
        expect(normalizeTerminalThemeId("user:")).toBe("match-gui");
      });

      it("resolveTerminalTheme returns the user palette when present", () => {
        expect(resolveTerminalTheme("deep-blue", "user:my-fav", [userTheme])).toEqual(
          userTheme.palette,
        );
      });

      it("resolveTerminalTheme falls back to GUI palette when the user theme is missing", () => {
        // ID was persisted but the file is currently absent — render GUI palette
        // rather than crash so the user can fix or pick a different theme.
        expect(resolveTerminalTheme("deep-blue", "user:gone", [])).toEqual(
          getTerminalTheme("deep-blue"),
        );
      });

      it("getAllTerminalThemeDefinitions appends a User group when non-empty", () => {
        const withoutUser = getAllTerminalThemeDefinitions([]);
        const withUser = getAllTerminalThemeDefinitions([userTheme]);
        expect(withUser.length).toBe(withoutUser.length + 1);
        const last = withUser[withUser.length - 1];
        expect(last.id).toBe("user:my-fav");
        expect(last.category).toBe("user");
      });
    });
  });
});
