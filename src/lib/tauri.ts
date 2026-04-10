import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Session,
  Project,
  RouxSettings,
  Worktree,
  SessionStatusPayload,
  TaskGroup,
  ClaudeSession,
  Watch,
  CreateWatchConfig,
  WatchUpdateEvent,
} from "./types";
import { ptyOutputPayloadToBytes, type PtyOutputPayload } from "./ptyOutput";
export type { PtyOutputPayload } from "./ptyOutput";

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

export async function reconnectSessionPty(
  id: string,
  extraFlags?: string[],
): Promise<Session> {
  return invoke("reconnect_session", { id, extraFlags: extraFlags ?? null });
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

export function createPtyOutputChannel(
  callback: (data: Uint8Array) => void
): Channel<PtyOutputPayload> {
  const channel = new Channel<PtyOutputPayload>();
  channel.onmessage = (payload) => {
    callback(ptyOutputPayloadToBytes(payload));
  };
  return channel;
}

export async function attachPtyOutput(
  id: string,
  onEvent: Channel<PtyOutputPayload>
): Promise<void> {
  return invoke("attach_pty_output", { id, onEvent });
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
export async function checkIsGitRepo(path: string): Promise<boolean> {
  return invoke("check_is_git_repo", { path });
}

export async function gitInit(path: string): Promise<void> {
  return invoke("git_init", { path });
}

export async function quitApp(): Promise<void> {
  return invoke("quit_app");
}

export async function refreshSessionGitStatus(id: string): Promise<boolean> {
  return invoke("refresh_session_git_status", { id });
}

export async function listBranches(repoPath: string): Promise<string[]> {
  return invoke("cmd_list_branches", { repoPath });
}

// Setup / CLI install
import type { SetupStatus } from "$lib/bindings";
export type { SetupStatus };

export async function checkSetupNeeded(): Promise<boolean> {
  return invoke("check_setup_needed");
}

export async function checkSetupStatus(): Promise<SetupStatus> {
  return invoke("check_setup_status");
}

export async function runSetup(): Promise<void> {
  return invoke("run_setup");
}

// Nono sandbox integration
export async function checkNonoInstalled(): Promise<boolean> {
  return invoke("check_nono_installed");
}

export async function listNonoProfiles(): Promise<string[]> {
  return invoke("list_nono_profiles");
}

// Projects
export async function listProjects(): Promise<Project[]> {
  return invoke("list_projects");
}

export async function createProject(name: string): Promise<Project> {
  return invoke("create_project", { name });
}

export async function removeProject(id: string): Promise<void> {
  return invoke("remove_project", { id });
}

export async function renameProject(id: string, name: string): Promise<void> {
  return invoke("rename_project", { id, name });
}

export async function setSessionProject(
  sessionId: string,
  projectId: string | null,
): Promise<void> {
  return invoke("set_session_project", { sessionId, projectId });
}

// Project notes
export async function getProjectNotes(projectId: string): Promise<string> {
  return invoke("get_project_notes", { projectId });
}

export async function setProjectNotes(projectId: string, content: string): Promise<void> {
  return invoke("set_project_notes", { projectId, content });
}

// Editor integration
export async function openInEditor(path: string): Promise<void> {
  return invoke("cmd_open_in_editor", { path });
}

// Document viewer commands
import type { DocFile } from "$lib/bindings";
export type { DocFile };

export async function readFile(path: string): Promise<string> {
  return invoke("read_file", { path });
}

export async function writeFile(path: string, contents: string): Promise<void> {
  return invoke("write_file", { path, contents });
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

export interface SessionExitPayload {
  code: number | null;
  generation?: number;
  reason?: "exit" | "io_error" | "killed";
}

export function onSessionExit(
  sessionId: string,
  callback: (payload: SessionExitPayload) => void
): Promise<UnlistenFn> {
  return listen<SessionExitPayload>(
    `session-exit:${sessionId}`,
    (event) => {
      callback(event.payload);
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

export interface RouxCommand {
  action: string;
  sessionId?: string;
  paneId?: string;
  ptyId?: string;
  direction?: string;
  command?: string;
  workingDir?: string;
}

export function onRouxCommand(
  callback: (payload: RouxCommand) => void
): Promise<UnlistenFn> {
  return listen<RouxCommand>("roux-command", (event) => {
    callback(event.payload);
  });
}

// Watch commands
export async function createWatch(config: CreateWatchConfig): Promise<Watch> {
  return invoke("cmd_create_watch", { config });
}

export async function removeWatch(id: string): Promise<void> {
  return invoke("cmd_remove_watch", { id });
}

export async function listWatches(): Promise<Watch[]> {
  return invoke("cmd_list_watches");
}

export async function pauseWatch(id: string): Promise<void> {
  return invoke("cmd_pause_watch", { id });
}

export async function resumeWatch(id: string): Promise<void> {
  return invoke("cmd_resume_watch", { id });
}

// Watch events
export function onWatchUpdate(
  callback: (payload: WatchUpdateEvent) => void
): Promise<UnlistenFn> {
  return listen<WatchUpdateEvent>("watch-update", (event) => {
    callback(event.payload);
  });
}
