import type { Channel } from "@tauri-apps/api/core";
import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal, IDisposable } from "@xterm/xterm";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { killSession, type PtyOutputPayload } from "$lib/tauri";

interface BaseTerminalEntry {
  terminal: Terminal;
  fitAddon: FitAddon | null;
  unlisteners: UnlistenFn[];
  disposables: IDisposable[];
  outputChannel: Channel<PtyOutputPayload> | null;
  generation: number | null;
}

export interface ClaudeTerminalEntry extends BaseTerminalEntry {}

export interface ShellTerminalEntry extends BaseTerminalEntry {
  ptyId: string;
}

const claudeTerminals = new Map<string, ClaudeTerminalEntry>();
const shellTerminals = new Map<string, ShellTerminalEntry>();

export function ensureClaudeTerminal(
  sessionId: string,
  create: () => ClaudeTerminalEntry
): ClaudeTerminalEntry {
  const existing = claudeTerminals.get(sessionId);
  if (existing) return existing;
  const entry = create();
  claudeTerminals.set(sessionId, entry);
  return entry;
}

export function ensureShellTerminal(
  paneId: string,
  create: () => ShellTerminalEntry
): ShellTerminalEntry {
  const existing = shellTerminals.get(paneId);
  if (existing) return existing;
  const entry = create();
  shellTerminals.set(paneId, entry);
  return entry;
}

function disposeEntry(entry: BaseTerminalEntry) {
  for (const unlisten of entry.unlisteners.splice(0)) {
    unlisten();
  }
  for (const disposable of entry.disposables.splice(0)) {
    disposable.dispose();
  }
  entry.outputChannel = null;
  entry.terminal.dispose();
  entry.fitAddon = null;
}

export async function disposeClaudeTerminal(sessionId: string) {
  const entry = claudeTerminals.get(sessionId);
  if (!entry) return;
  disposeEntry(entry);
  claudeTerminals.delete(sessionId);
}

export async function disposeShellTerminal(paneId: string) {
  const entry = shellTerminals.get(paneId);
  if (!entry) return;
  disposeEntry(entry);
  shellTerminals.delete(paneId);
  await killSession(entry.ptyId).catch(() => {});
}
