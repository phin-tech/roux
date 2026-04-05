import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Session,
  RouxSettings,
  Worktree,
  SessionStatusPayload,
  TaskGroup,
  ClaudeSession,
} from "./types";

// Commands (frontend → backend)
export async function createSession(
  repoPath: string,
  name: string,
  worktreePath: string | null,
  branch: string | null,
  extraFlags?: string[],
  nonoProfile?: string | null,
): Promise<Session> {
  return invoke("create_session", {
    repoPath,
    name,
    worktreePath,
    branch,
    extraFlags: extraFlags ?? null,
    nonoProfile: nonoProfile ?? null,
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

export async function spawnShell(id: string, workingDir: string): Promise<void> {
  return invoke("spawn_shell", { id, workingDir });
}

export async function spawnTask(id: string, command: string, workingDir: string): Promise<void> {
  return invoke("spawn_task", { id, command, workingDir });
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

// Claude sessions
export async function listClaudeSessions(cwd: string): Promise<ClaudeSession[]> {
  return invoke("list_claude_sessions", { cwd });
}

// Git
export async function listBranches(repoPath: string): Promise<string[]> {
  return invoke("cmd_list_branches", { repoPath });
}

// Nono sandbox integration
export async function checkNonoInstalled(): Promise<boolean> {
  return invoke("check_nono_installed");
}

export async function listNonoProfiles(): Promise<string[]> {
  return invoke("list_nono_profiles");
}

// Editor integration
export async function openInEditor(path: string): Promise<void> {
  return invoke("cmd_open_in_editor", { path });
}

// Document viewer commands
export interface DocFile {
  path: string;
  name: string;
  relativePath: string;
  modified: number;
}

export async function readFile(path: string): Promise<string> {
  return invoke("read_file", { path });
}

export async function listDocs(dir: string): Promise<DocFile[]> {
  return invoke("list_docs", { dir });
}

// Task discovery
export async function discoverTasks(dir: string): Promise<TaskGroup[]> {
  return invoke("cmd_discover_tasks", { dir });
}

export async function loadTaskOverrides(): Promise<Record<string, Record<string, string>>> {
  return invoke("cmd_load_task_overrides");
}

export async function saveTaskOverrides(
  overrides: Record<string, Record<string, string>>
): Promise<void> {
  return invoke("cmd_save_task_overrides", { overrides });
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
