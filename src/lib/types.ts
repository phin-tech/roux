export type ThemePreset = "deep-blue" | "steel-amber" | "slate-emerald" | "graphite-rose";

export interface PermissionInfo {
  toolName: string;
  toolInput: Record<string, any>;
  message: string;
}

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
  permissionInfo: PermissionInfo | null;
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
  theme: ThemePreset;
  defaultModel: string | null;
  additionalFlags: string[];
  taskPanelSplit: number;
  taskPanelCollapsed: boolean;
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
  fontFamily: "JetBrains Mono, IBM Plex Mono, SFMono-Regular, monospace",
  lineHeight: 1.2,
  scrollback: 5000,
  cursorStyle: "block",
  cursorBlink: true,
  defaultProjectPath: null,
  confirmOnClose: true,
  restoreSessionsOnLaunch: true,
  worktreeBasePath: null,
  cleanupWorktreesOnClose: false,
  theme: "deep-blue",
  defaultModel: null,
  additionalFlags: [],
  taskPanelSplit: 0.4,
  taskPanelCollapsed: false,
};

export type { KeepOpen, TaskDefinition, TaskGroup, TaskRun } from "./types/tasks";
