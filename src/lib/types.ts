export interface Session {
  id: string;
  name: string;
  repoRoot: string;
  worktreePath: string;
  branch: string;
  isWorktree: boolean;
  status: "idle" | "thinking" | "generating" | "error" | "disconnected" | "attention";
  model: string | null;
  cost: number | null;
  createdAt: number;
}

export interface RouxSettings {
  tabPosition: "left" | "right";
  tabWidth: number;
  fontSize: number;
  fontFamily: string;
  lineHeight: number;
  scrollback: number;
  cursorStyle: "block" | "underline" | "bar";
  cursorBlink: boolean;
  defaultProjectPath: string | null;
  confirmOnClose: boolean;
  restoreSessionsOnLaunch: boolean;
  worktreeBasePath: string | null;
  cleanupWorktreesOnClose: boolean;
  theme: "dark";
  defaultModel: string | null;
  additionalFlags: string[];
}

export interface Worktree {
  path: string;
  branch: string;
  isMain: boolean;
}

export interface SessionStatusPayload {
  status: string;
  model: string | null;
  cost: number | null;
}

export const DEFAULT_SETTINGS: RouxSettings = {
  tabPosition: "left",
  tabWidth: 260,
  fontSize: 14,
  fontFamily: "IBM Plex Mono, monospace",
  lineHeight: 1.2,
  scrollback: 5000,
  cursorStyle: "block",
  cursorBlink: true,
  defaultProjectPath: null,
  confirmOnClose: true,
  restoreSessionsOnLaunch: true,
  worktreeBasePath: null,
  cleanupWorktreesOnClose: false,
  theme: "dark",
  defaultModel: null,
  additionalFlags: [],
};
