import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Session,
  RouxSettings,
  Worktree,
  SessionStatusPayload,
} from "./types";

// Commands (frontend → backend)
export async function createSession(
  repoPath: string,
  name: string,
  worktreePath: string | null,
  branch: string | null
): Promise<Session> {
  return invoke("create_session", {
    repoPath,
    name,
    worktreePath,
    branch,
  });
}

export async function killSession(id: string): Promise<void> {
  return invoke("kill_session", { id });
}

export async function writeToSession(
  id: string,
  data: string
): Promise<void> {
  return invoke("write_to_session", { id, data });
}

export async function resizeSession(
  id: string,
  cols: number,
  rows: number
): Promise<void> {
  return invoke("resize_session", { id, cols, rows });
}

export async function listSessions(): Promise<Session[]> {
  return invoke("list_sessions");
}

export async function getSettings(): Promise<RouxSettings> {
  return invoke("get_settings");
}

export async function updateSettings(
  settings: RouxSettings
): Promise<void> {
  return invoke("update_settings", { settings });
}

export async function createWorktree(
  repoPath: string,
  branch: string
): Promise<string> {
  return invoke("cmd_create_worktree", { repoPath, branch });
}

export async function removeWorktree(
  worktreePath: string
): Promise<void> {
  return invoke("cmd_remove_worktree", { worktreePath });
}

export async function listWorktrees(
  repoPath: string
): Promise<Worktree[]> {
  return invoke("cmd_list_worktrees", { repoPath });
}

// Events (backend → frontend)
export function onPtyOutput(
  sessionId: string,
  callback: (data: string) => void
): Promise<UnlistenFn> {
  return listen<string>(`pty-output:${sessionId}`, (event) => {
    callback(event.payload);
  });
}

export function onSessionStatus(
  sessionId: string,
  callback: (payload: SessionStatusPayload) => void
): Promise<UnlistenFn> {
  return listen<SessionStatusPayload>(
    `session-status:${sessionId}`,
    (event) => {
      callback(event.payload);
    }
  );
}

export function onSessionExit(
  sessionId: string,
  callback: (code: number | null) => void
): Promise<UnlistenFn> {
  return listen<{ code: number | null }>(
    `session-exit:${sessionId}`,
    (event) => {
      callback(event.payload.code);
    }
  );
}

export function onSettingsChanged(
  callback: (settings: RouxSettings) => void
): Promise<UnlistenFn> {
  return listen<RouxSettings>("settings-changed", (event) => {
    callback(event.payload);
  });
}

export interface StatusUpdate {
  status: string;
  cwd: string;
  claudeSessionId: string;
  toolName: string | null;
  toolInput: Record<string, any> | null;
  message: string | null;
}

export function onRouxStatusUpdate(
  callback: (payload: StatusUpdate) => void
): Promise<UnlistenFn> {
  return listen<StatusUpdate>("roux-status-update", (event) => {
    callback(event.payload);
  });
}
