import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SpawnProfileRef } from "$lib/panes/profiles";
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

export interface InitialPtySize {
  cols: number;
  rows: number;
}

export interface CreateSessionShellOpts {
  nonoProfile?: string | null;
  nonoAllowDirs?: string[] | null;
  initialSize?: InitialPtySize | null;
  /**
   * Spawn-profile id (`claude`, `codex`, user-profile id). Threaded into the
   * PTY env so agents wake up under the right profile.
   */
  profile?: string | null;
  /**
   * Git starting point for a newly-created worktree branch (e.g. "main",
   * "origin/main"). Only used when `branch` is a new branch; ignored for
   * existing branches.
   */
  base?: string | null;
  /** Run `git fetch origin` before resolving `base`. */
  fetchFirst?: boolean;
}

/**
 * Spawns a plain shell in the session's primary PTY. Caller then attaches
 * a spawn profile and types its setup / startup commands into the shell.
 *
 * `initialSize` seeds the PTY's cell dimensions to avoid a post-spawn
 * SIGWINCH that otherwise triggers `zle reset-prompt` in zsh and causes
 * async prompt frameworks to paint over typed input. Pass the pane's
 * estimated size when a pane is available.
 */
export async function createSessionShell(
  repoPath: string,
  name: string,
  worktreePath: string | null,
  branch: string | null,
  opts: CreateSessionShellOpts = {},
): Promise<Session> {
  const { nonoProfile, nonoAllowDirs, initialSize, profile, base, fetchFirst } = opts;
  return invoke("create_session_shell", {
    repoPath,
    name,
    worktreePath,
    branch,
    opts: {
      nonoProfile: nonoProfile ?? null,
      nonoAllowDirs: nonoAllowDirs ?? null,
      profile: profile ?? null,
      initialSize: initialSize ? [initialSize.cols, initialSize.rows] : null,
      base: base ?? null,
      fetchFirst: fetchFirst ?? null,
    },
  });
}

export async function killSession(id: string): Promise<void> {
  return invoke("kill_session", { id });
}

/**
 * Kill only the PTY `id`, leaving the session record and pane-state files
 * alone. Use this for pane disposal — `killSession` removes the session
 * too, which is almost never what a pane-level close wants.
 */
export async function killPty(id: string): Promise<void> {
  return invoke("kill_pty", { id });
}

/**
 * Respawns a plain shell in the session's primary PTY. The caller replays
 * the profile's setup / startup commands into the fresh shell after this
 * resolves.
 */
export async function reconnectSessionShellPty(
  id: string,
  nonoProfile?: string | null,
  nonoAllowDirs?: string[] | null,
  profile?: string | null,
  initialSize?: InitialPtySize | null,
): Promise<Session> {
  return invoke("reconnect_session_shell", {
    id,
    nonoProfile: nonoProfile ?? null,
    nonoAllowDirs: nonoAllowDirs ?? null,
    profile: profile ?? null,
    initialSize: initialSize ? [initialSize.cols, initialSize.rows] : null,
  });
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

export async function spawnShell(
  id: string,
  workingDir: string,
  sessionId: string | null,
  paneId: string | null,
  nonoProfile?: string | null,
  nonoAllowDirs?: string[] | null,
  profile?: string | null,
  initialSize?: InitialPtySize | null,
): Promise<void> {
  return invoke("spawn_shell", {
    id,
    workingDir,
    sessionId,
    paneId,
    nonoProfile: nonoProfile ?? null,
    nonoAllowDirs: nonoAllowDirs ?? null,
    profile: profile ?? null,
    initialSize: initialSize ? [initialSize.cols, initialSize.rows] : null,
  });
}

export async function spawnTask(
  id: string,
  command: string,
  workingDir: string,
  sessionId: string | null,
  paneId: string | null,
  profile?: string | null,
  initialSize?: InitialPtySize | null,
): Promise<void> {
  return invoke("spawn_task", {
    id,
    command,
    workingDir,
    sessionId,
    paneId,
    profile: profile ?? null,
    initialSize: initialSize ? [initialSize.cols, initialSize.rows] : null,
  });
}

export async function listSessions(): Promise<Session[]> {
  return invoke("list_sessions");
}

export async function listArchivedSessions(): Promise<Session[]> {
  return invoke("list_archived_sessions");
}

export async function restoreSession(id: string): Promise<void> {
  return invoke("restore_session", { id });
}

export async function deleteSessionPermanently(id: string): Promise<void> {
  return invoke("delete_session_permanently", { id });
}

export async function sessionWorktreeExists(id: string): Promise<boolean> {
  return invoke("session_worktree_exists", { id });
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
  branch: string,
  opts: { startPoint?: string | null; fetchFirst?: boolean } = {},
): Promise<string> {
  return invoke("cmd_create_worktree", {
    repoPath,
    branch,
    startPoint: opts.startPoint ?? null,
    fetchFirst: opts.fetchFirst ?? false,
  });
}

export async function removeWorktree(
  repoPath: string,
  worktreePath: string,
  alsoBranch: boolean = false
): Promise<void> {
  return invoke("cmd_remove_worktree", {
    repoPath,
    worktreePath,
    alsoBranch,
  });
}

export async function openPathInFinder(path: string): Promise<void> {
  return invoke("cmd_open_path_in_finder", { path });
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

export async function listGitReposInRoots(
  roots: string[],
  excludeWorktrees: boolean,
): Promise<string[]> {
  return invoke("list_git_repos_in_roots", { roots, excludeWorktrees });
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

// Doctor / integration health
import type { DoctorStatus, DoctorItem } from "$lib/bindings";
export type { DoctorStatus, DoctorItem };

export async function checkDoctorStatus(): Promise<DoctorStatus> {
  return invoke("check_doctor_status");
}

export async function reinstallCli(): Promise<void> {
  return invoke("reinstall_cli");
}

export async function reinstallHooks(): Promise<void> {
  return invoke("reinstall_hooks");
}

export async function reinstallSkill(): Promise<void> {
  return invoke("reinstall_skill");
}

export async function installAllMissing(): Promise<void> {
  return invoke("install_all_missing");
}

// Nono sandbox integration
export async function checkNonoInstalled(): Promise<boolean> {
  return invoke("check_nono_installed");
}

export async function listNonoProfiles(): Promise<string[]> {
  return invoke("list_nono_profiles");
}

// GitHub PR integration
export interface PrInfo {
  number: number;
  title: string;
  headRef: string;
  headOwner: string;
  isCrossRepository: boolean;
}

export async function checkGhInstalled(): Promise<boolean> {
  return invoke("check_gh_installed");
}

export async function lookupPr(repoPath: string | null, url: string): Promise<PrInfo> {
  return invoke("lookup_pr", { repoPath, url });
}

export async function cloneRepo(
  owner: string,
  repo: string,
  targetDir: string,
): Promise<string> {
  return invoke("clone_repo", { owner, repo, targetDir });
}

export async function fetchPrBranch(
  repoPath: string,
  number: number,
  headRef: string,
  isCrossRepository: boolean,
): Promise<string> {
  return invoke("fetch_pr_branch", { repoPath, number, headRef, isCrossRepository });
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

export async function setSessionNameOverride(
  sessionId: string,
  nameOverride: string | null,
): Promise<void> {
  return invoke("set_session_name_override", { sessionId, nameOverride });
}

// Multi-scoped notes (experimental — see docs/features/notes.md). The four
// scopes are `"global" | "project" | "repo" | "session"`. Every command
// routes through NotesService on the backend; scope resolution (repo slug,
// project slug, session slug) happens there.
export type NotesScope = "global" | "project" | "repo" | "session";

export interface NotesTarget {
  scope: NotesScope;
  sessionId?: string | null;
  topic?: string | null;
  overrideSlug?: string | null;
}

export interface NotesRead {
  path: string;
  content: string;
}

export interface NotesSearchQuery {
  tags: string[];
  scope?: NotesScope | null;
  exact: boolean;
}

export async function notesRead(target: NotesTarget): Promise<NotesRead> {
  return invoke("notes_read", { target });
}

export async function notesWrite(
  target: NotesTarget,
  content: string,
  tags: string[] = [],
): Promise<void> {
  return invoke("notes_write", { target, content, tags });
}

export async function notesAppend(
  target: NotesTarget,
  content: string,
  timestamped: boolean,
  tags: string[] = [],
): Promise<void> {
  return invoke("notes_append", { target, content, timestamped, tags });
}

export async function notesPath(target: NotesTarget, dir: boolean): Promise<string> {
  return invoke("notes_path", { target, dir });
}

export async function notesSearch(query: NotesSearchQuery): Promise<string[]> {
  return invoke("notes_search", { query });
}

export async function notesVaultRoot(): Promise<string> {
  return invoke("notes_vault_root");
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
  /**
   * Provider-internal session id (Claude's `session_id`, Codex's equivalent,
   * etc.). `null` when the hook didn't carry one. Formerly `claudeSessionId`
   * — renamed so non-Claude hooks don't have to masquerade as Claude.
   */
  providerSessionId: string | null;
  /** Provider that emitted the hook (e.g. `"claude"`). Empty string for legacy payloads. */
  provider: string;
  /** Roux session id captured from `ROUX_SESSION_ID` at hook time. */
  rouxSessionId: string | null;
  /** Roux pane id captured from `ROUX_PANE_ID` at hook time. Tier-1 routing key. */
  rouxPaneId: string | null;
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

/**
 * Emitted by the backend agent FSM when a pane-scoped agent leaves the
 * `Attention` state (user answered, agent crashed, session ended). The
 * notification auto-dismiss is already handled server-side via the
 * notification store; this event exists purely so the frontend can
 * clear any stale `permissionInfo` sitting in the pane's `AgentState`
 * (Allow/Deny UI etc.) alongside the notification disappearance. Gated
 * server-side on the `autoClearAttentionState` setting.
 */
export interface AttentionClearedEvent {
  paneId: string;
}

export function onAgentAttentionCleared(
  callback: (payload: AttentionClearedEvent) => void
): Promise<UnlistenFn> {
  return listen<AttentionClearedEvent>("agent-attention-cleared", (event) => {
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
  profileId?: string;
  requestId?: string;
}

/**
 * Reply to a socket-initiated request/response round-trip. The `requestId`
 * comes from the matching `roux-command` event; `data` is the JSON payload
 * the waiting CLI caller will receive.
 */
export async function submitRouxReply(
  requestId: string,
  data: unknown,
): Promise<void> {
  return invoke("submit_roux_reply", { requestId, data });
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

// Notification events
import type { NotificationEvent } from "./types/notifications";
import type { Notification, NotificationRequest, NotificationSource } from "./bindings";

export function onNotificationEvent(
  callback: (payload: NotificationEvent) => void
): Promise<UnlistenFn> {
  return listen<NotificationEvent>("notification-event", (event) => {
    callback(event.payload);
  });
}

// Notification commands — thin wrappers that unwrap the typed-error envelope.
// Errors are surfaced as rejected promises so callers can try/catch as usual.
async function unwrap<T>(
  p: Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>,
): Promise<T> {
  const r = await p;
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}

export async function listNotifications(): Promise<Notification[]> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsList());
}

export async function listNotificationsForSession(
  sessionId: string | null,
): Promise<Notification[]> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsListForSession(sessionId));
}

export async function notificationsMarkRead(id: string): Promise<boolean> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsMarkRead(id));
}

export async function notificationsMarkAllRead(
  sessionId: string | null,
  global: boolean | null,
): Promise<number> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsMarkAllRead(sessionId, global));
}

export async function notificationsRemove(id: string): Promise<boolean> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsRemove(id));
}

export async function notificationsClear(
  sessionId: string | null,
  global: boolean | null,
): Promise<number> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsClear(sessionId, global));
}

export async function notificationsDismissSource(
  source: NotificationSource,
): Promise<number> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsDismissSource(source));
}

export async function notificationsPush(
  request: NotificationRequest,
): Promise<Notification> {
  const { commands } = await import("./bindings");
  return unwrap(commands.notificationsPush(request));
}

export async function getPtyCwd(id: string): Promise<string | null> {
  return invoke("get_pty_cwd", { id });
}

// ── Pane state persistence ────────────────────────────────────────────────────

export async function loadPaneStateRaw(sessionId: string): Promise<unknown | null> {
  return invoke("load_pane_state", { sessionId });
}

export async function savePaneStateRaw(sessionId: string, data: unknown): Promise<void> {
  return invoke("save_pane_state", { sessionId, data });
}

export async function saveLivePaneStateRaw(
  sessionId: string,
  schemaVersion: number,
  layout: unknown,
  paneIds: string[],
): Promise<void> {
  return invoke("save_live_pane_state", {
    sessionId,
    schemaVersion,
    layout,
    paneIds,
  });
}

export async function deletePaneStateRaw(sessionId: string): Promise<void> {
  return invoke("delete_pane_state", { sessionId });
}

// ── PTY attach / detach ───────────────────────────────────────────────────────

export type { AttachResult, PtyInfo, PtyRole, PtyStatus } from "./bindings";

export async function attachPtyToPane(
  ptyId: string,
  paneId: string,
  cols: number,
  rows: number,
): Promise<import("./bindings").AttachResult> {
  return invoke("attach_pty_to_pane", { ptyId, paneId, cols, rows });
}

export async function detachPty(ptyId: string): Promise<void> {
  return invoke("detach_pty", { ptyId });
}

export async function listSessionPtys(sessionId: string): Promise<import("./bindings").PtyInfo[]> {
  return invoke("list_session_ptys", { sessionId });
}

export async function markPtyRead(ptyId: string): Promise<void> {
  return invoke("mark_pty_read", { ptyId });
}

export async function setPtyName(ptyId: string, name: string | null): Promise<void> {
  return invoke("set_pty_name", { ptyId, name });
}

export interface PaneDescriptorPayload {
  id: string;
  type: "shell" | "markdown" | "command" | "notes";
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
  spawnProfileRef?: SpawnProfileRef;
  nonoProfile?: string;
  nonoAllowDirs?: string[];
  notesScope?: NotesScope;
  notesViewMode?: "edit" | "read";
}

export type PaneRecordPayload = PaneDescriptorPayload;

export async function upsertPaneRecord(record: PaneRecordPayload): Promise<void> {
  return invoke("upsert_pane_record", { record });
}

export async function removePaneRecord(id: string): Promise<void> {
  return invoke("remove_pane_record", { id });
}
