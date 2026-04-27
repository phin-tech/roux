import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { openUrl } from "@tauri-apps/plugin-opener";
import { get } from "svelte/store";

import { settings } from "$lib/stores/settings";
import { userTerminalThemes } from "$lib/stores/userTerminalThemes";
import { resolveTerminalTheme, type TerminalTheme } from "$lib/themes";

import { installXtermWatchDecorations } from "./xtermWatchDecorations";
import { readPromptSnapshot, type PromptSnapshot } from "./promptSnapshot";
import type { TerminalController, TerminalDimensions } from "./terminalRuntime";

interface CreateTerminalControllerOptions {
  allowKeyboardEvent?: (event: KeyboardEvent) => boolean;
}

class XtermTerminalController implements TerminalController {
  private readonly terminal: Terminal;
  private readonly fitAddon: FitAddon;

  constructor(options?: CreateTerminalControllerOptions) {
    const s = get(settings);
    this.terminal = new Terminal({
      fontSize: s.fontSize,
      fontFamily: s.fontFamily,
      lineHeight: s.lineHeight,
      scrollback: s.scrollback,
      cursorStyle: s.cursorStyle as "block" | "underline" | "bar",
      cursorBlink: s.cursorBlink,
      theme: toXtermTheme(resolveTerminalTheme(s.theme, s.terminalTheme, get(userTerminalThemes))),
      disableStdin: true,
      allowProposedApi: true,
    });

    this.fitAddon = new FitAddon();
    this.terminal.loadAddon(this.fitAddon);
    try {
      this.terminal.loadAddon(new WebglAddon());
    } catch {
      // WebGL not available — canvas fallback
    }

    if (options?.allowKeyboardEvent) {
      this.setCustomKeyHandler(options.allowKeyboardEvent);
    }

    this.terminal.loadAddon(new WebLinksAddon((_event, uri) => {
      openUrl(uri);
    }));

    installXtermWatchDecorations(this.terminal);
  }

  attach(container: HTMLElement): void {
    if (!this.terminal.element) {
      this.terminal.open(container);
      try {
        (container.querySelector(".xterm-helper-textarea") as HTMLElement | null)
          ?.blur();
      } catch {
        // best-effort
      }
    } else if (!container.contains(this.terminal.element)) {
      container.appendChild(this.terminal.element);
    }
    // Defer fit to next frame so the container has layout dimensions.
    // Immediate fit can measure 0x0 or stale sizes before CSS layout runs,
    // causing PTY to stay at 80x24 default.
    requestAnimationFrame(() => this.fit());
  }

  detach(): void {
    const el = this.terminal.element;
    if (el?.parentElement) {
      el.parentElement.removeChild(el);
    }
  }

  dispose(): void {
    this.terminal.dispose();
  }

  clear(): void {
    this.terminal.clear();
  }

  reset(): void {
    this.terminal.reset();
  }

  fit(): TerminalDimensions | null {
    this.fitAddon.fit();
    const dims = this.fitAddon.proposeDimensions();
    return dims ? { cols: dims.cols, rows: dims.rows } : null;
  }

  setInputEnabled(enabled: boolean): void {
    this.terminal.options.disableStdin = !enabled;
  }

  onInput(handler: (data: string) => void): () => void {
    const disposable = this.terminal.onData(handler);
    return () => disposable.dispose();
  }

  write(bytes: Uint8Array): void {
    this.terminal.write(bytes);
  }

  focus(): void {
    this.terminal.focus();
  }

  setTheme(theme: TerminalTheme): void {
    this.terminal.options.theme = toXtermTheme(theme);
  }

  setCustomKeyHandler(handler: ((event: KeyboardEvent) => boolean) | null): void {
    this.terminal.attachCustomKeyEventHandler((event) => {
      if (!handler) return true;
      return handler(event);
    });
  }

  getPromptSnapshot(): PromptSnapshot | null {
    return readPromptSnapshot(this.terminal.buffer.active);
  }

  isNewShell(): boolean {
    return this.terminal.buffer.active.length < 5;
  }
}

function toXtermTheme(theme: TerminalTheme): ITheme {
  return {
    background: theme.background,
    foreground: theme.foreground,
    cursor: theme.cursor,
    selectionBackground: theme.selectionBackground,
    black: theme.ansi.black,
    red: theme.ansi.red,
    green: theme.ansi.green,
    yellow: theme.ansi.yellow,
    blue: theme.ansi.blue,
    magenta: theme.ansi.magenta,
    cyan: theme.ansi.cyan,
    white: theme.ansi.white,
    brightBlack: theme.ansi.brightBlack,
    brightRed: theme.ansi.brightRed,
    brightGreen: theme.ansi.brightGreen,
    brightYellow: theme.ansi.brightYellow,
    brightBlue: theme.ansi.brightBlue,
    brightMagenta: theme.ansi.brightMagenta,
    brightCyan: theme.ansi.brightCyan,
    brightWhite: theme.ansi.brightWhite,
  };
}

export function createXtermTerminalController(
  options?: CreateTerminalControllerOptions,
): TerminalController {
  return new XtermTerminalController(options);
}
