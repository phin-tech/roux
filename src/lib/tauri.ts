import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SpawnProfileRef } from "$lib/panes/profiles";
import type {
  Session,
  Project,
  ProjectUpdate,
  RouxSettings,
  Worktree,
  SessionStatusPayload,
  TaskGroup,
  ClaudeSession,
  Watch,
  CreateWatchConfig,
  WatchUpdateEvent,
  SmolMachine,
  SmolMachineCreateRequest,
  ManagedProxyStatus,
  WorktreeMountCheck,
  MountAppendOutcome,
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
  /**
   * Project to attach the new session to. When set, the PTY env vars for
   * project notes + ROUX_PROJECT_CONTEXT_PATHS land on the very first spawn.
   */
  projectId?: string | null;
  /**
   * Project session-blueprint id this session was spawned from. Stamped on
   * the persisted Session so the sidebar can collapse the dimmed blueprint
   * row when the live session is up.
   */
  blueprintId?: string | null;
  /**
   * Smol-machine binding. When set (and non-empty), the session's primary
   * PTY and every subsequent shell pane runs inside the named VM via
   * `smolvm machine exec`. Empty/null leaves the session running on host.
   */
  smolMachineName?: string | null;
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
  const {
    nonoProfile,
    nonoAllowDirs,
    initialSize,
    profile,
    base,
    fetchFirst,
    projectId,
    blueprintId,
    smolMachineName,
  } = opts;
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
      projectId: projectId ?? null,
      blueprintId: blueprintId ?? null,
      smolMachineName: smolMachineName ?? null,
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
  alsoBranch: boolean = false,
  force: boolean = false
): Promise<void> {
  return invoke("cmd_remove_worktree", {
    repoPath,
    worktreePath,
    alsoBranch,
    force,
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

// Smol machines: thin wrappers around the typed-error commands so panel
// code can `await` and `try/catch` instead of branching on the
// discriminated `{ status: "ok" } | { status: "error" }` shape.
export async function listSmolMachines(): Promise<SmolMachine[]> {
  return invoke("cmd_list_smol_machines");
}

export async function startSmolMachine(name: string): Promise<void> {
  return invoke("cmd_start_smol_machine", { name });
}

export async function stopSmolMachine(name: string): Promise<void> {
  return invoke("cmd_stop_smol_machine", { name });
}

export async function deleteSmolMachine(name: string): Promise<void> {
  return invoke("cmd_delete_smol_machine", { name });
}

export async function createSmolMachine(
  request: SmolMachineCreateRequest,
): Promise<void> {
  return invoke("cmd_create_smol_machine", { request });
}

/**
 * `which`-style probe inside a smol guest. Resolves to the guest path
 * when the binary is on PATH, `null` when it isn't, or rejects when
 * the smolvm CLI itself fails (e.g. machine isn't running).
 */
export async function checkSmolvmBinary(
  machineName: string,
  binary: string,
): Promise<string | null> {
  return invoke("cmd_check_smolvm_binary", { machineName, binary });
}

/**
 * Install a known agent inside a smol guest. v1 supports "claude" and
 * "codex" (case-insensitive). Resolves on success; rejects with the
 * install command's stderr when smolvm or npm fail.
 */
export async function installSmolvmAgent(
  machineName: string,
  agent: "claude" | "codex",
): Promise<void> {
  return invoke("cmd_install_smolvm_agent", { machineName, agent });
}

/**
 * Persist an agent install via the machine's linked Smolfile
 * `[dev].init`. Returns a discriminated outcome:
 * - `appended` / `alreadyPresent`: file was (or already was) updated.
 * - `needsRecreate`: machine has no Smolfile linked. The frontend
 *   shows a confirmation modal and calls
 *   `installSmolvmAgentRecreate` if the user agrees.
 */
export type SmolvmPersistOutcome =
  | { kind: "appended"; smolfilePath: string }
  | { kind: "alreadyPresent"; smolfilePath: string }
  | {
      kind: "needsRecreate";
      proposedSmolfilePath: string;
      image: string | null;
      script: string;
    };

export async function installSmolvmAgentPersist(
  machineName: string,
  agent: "claude" | "codex",
): Promise<SmolvmPersistOutcome> {
  return invoke("cmd_install_smolvm_agent_persist", { machineName, agent });
}

/**
 * Destructive: regenerate the machine from a Roux-managed Smolfile
 * with the agent's install line in `[dev].init`. Only call after the
 * user confirms the modal triggered by a `needsRecreate` outcome.
 */
export async function installSmolvmAgentRecreate(
  machineName: string,
  agent: "claude" | "codex",
): Promise<void> {
  return invoke("cmd_install_smolvm_agent_recreate", { machineName, agent });
}

/**
 * Map of `{ machine_name: smolfile_path }` for machines Roux has
 * tracked as "Smolfile-linked" (created via the form with a Smolfile,
 * or recreated via the persist flow). Used to render the in-place vs.
 * recreate hint in the panel before the user clicks.
 */
export async function listSmolMachineSmolfiles(): Promise<
  Record<string, string>
> {
  return invoke("cmd_list_smol_machine_smolfiles");
}

/**
 * Open `~/.config/roux/smolvm-bootstraps.toml` in the user's default
 * editor. Creates the file with built-in defaults pre-populated when
 * it doesn't yet exist. Resolves to the absolute path that was
 * opened, so callers can show it in a tooltip / toast.
 */
export async function openSmolvmBootstrapConfig(): Promise<string> {
  return invoke("cmd_open_smolvm_bootstrap_config");
}

/**
 * Start the user-configured managed HTTP proxy. Errors when
 * `RouxSettings.managedProxy` isn't set, when the configured
 * command fails to spawn, or when the listen socket isn't
 * reachable within ~5s.
 */
export async function startManagedProxy(): Promise<ManagedProxyStatus> {
  return invoke("cmd_start_managed_proxy");
}

export async function stopManagedProxy(): Promise<ManagedProxyStatus> {
  return invoke("cmd_stop_managed_proxy");
}

export async function managedProxyStatus(): Promise<ManagedProxyStatus> {
  return invoke("cmd_managed_proxy_status");
}

/**
 * Check whether `worktreePath` is reachable inside `machineName`'s
 * guest. Returns `mounted` when an existing `[dev].volumes` entry
 * covers the path (host-side path prefix), `notMounted` when the
 * Smolfile exists but no spec covers it (with a proposed same-path
 * mount), or `noLinkedSmolfile` when Roux can't auto-mount because
 * it has no Smolfile to edit.
 */
export async function checkWorktreeMount(
  machineName: string,
  worktreePath: string,
): Promise<WorktreeMountCheck> {
  return invoke("cmd_check_worktree_mount", { machineName, worktreePath });
}

/**
 * Append `spec` to the linked Smolfile's `[dev].volumes`. Idempotent.
 * Errors when the machine has no linked Smolfile.
 *
 * The mount only takes effect on the next `smolvm machine create` for
 * the machine — the caller should inform the user that a recreate is
 * needed (or trigger one via the existing recreate flow).
 */
export async function appendWorktreeMount(
  machineName: string,
  spec: string,
): Promise<MountAppendOutcome> {
  return invoke("cmd_append_worktree_mount", { machineName, spec });
}

export type LibraryItemType = "prompt" | "skill" | "smolvmScript";
export type LibraryLayerKind = "global" | "localRepo" | "gitRepo" | "activeRepo";
export type LibrarySourceKind = "localRepo" | "gitRepo";
export type LibraryRemoteState = "upToDate" | "ahead" | "behind" | "diverged" | "unknown";
export type LibraryVariableType = "string" | "int" | "float" | "select";

export type SkillSyncMode = "off" | "copy" | "symlink";

export interface LibrarySource {
  id: string;
  kind: LibrarySourceKind;
  name: string;
  enabled: boolean;
  order: number;
  path?: string | null;
  url?: string | null;
  branch?: string | null;
  /** Per-source override for skill sync. `null` = inherit the global default. */
  skillSync?: SkillSyncMode | null;
}

export interface LibraryVariable {
  name: string;
  label?: string | null;
  default?: string | null;
  required: boolean;
  valueType?: LibraryVariableType;
  options?: string[];
}

export interface LibraryItem {
  id: string;
  itemType: LibraryItemType;
  title: string;
  description?: string | null;
  tags: string[];
  provider?: string | null;
  sourceLayer: LibraryLayerKind;
  sourceId?: string | null;
  sourceLabel: string;
  sourcePath: string;
  overriddenPaths: string[];
  variables: LibraryVariable[];
  /**
   * smolvmScript-only: the agent this script installs (`claude` /
   * `codex`). Undefined for prompts and skills.
   */
  agent?: string | null;
  /**
   * smolvmScript-only: distro key matching the install resolver
   * (`alpine` / `ubuntu` / `default`). Undefined for prompts and
   * skills.
   */
  distro?: string | null;
}

export interface LibraryRead {
  item: LibraryItem;
  body: string;
}

export interface RenderLibraryPromptRequest {
  itemId: string;
  variables: Record<string, string>;
  sessionId?: string | null;
}

export interface RenderedLibraryPrompt {
  itemId: string;
  content: string;
}

export type SaveLibraryTarget =
  | { type: "global" }
  | { type: "source"; id: string }
  | { type: "activeRepo" };

export interface SaveLibraryItemRequest {
  originalId?: string | null;
  itemId: string;
  itemType: LibraryItemType;
  title: string;
  description?: string | null;
  tags: string[];
  provider?: string | null;
  variables: LibraryVariable[];
  body: string;
  target: SaveLibraryTarget;
  expectedSourcePath?: string | null;
  /** smolvmScript-only. */
  agent?: string | null;
  /** smolvmScript-only. */
  distro?: string | null;
}

export interface SavedLibraryItem {
  itemId: string;
  sourcePath: string;
}

export interface LibraryGitStatus {
  sourceId: string;
  checkedOut: boolean;
  checkoutPath: string;
  branch: string;
  trackingBranch?: string | null;
  defaultBranch?: string | null;
  dirty: boolean;
  remoteState: LibraryRemoteState;
  ahead: number;
  behind: number;
  behindDefault?: number | null;
  error?: string | null;
}

export async function listLibraryItems(sessionId?: string | null): Promise<LibraryItem[]> {
  return invoke("list_library_items", { sessionId: sessionId ?? null });
}

export async function readLibraryItem(
  itemId: string,
  sessionId?: string | null,
): Promise<LibraryRead> {
  return invoke("read_library_item", { itemId, sessionId: sessionId ?? null });
}

export async function renderLibraryPrompt(
  request: RenderLibraryPromptRequest,
): Promise<RenderedLibraryPrompt> {
  return invoke("render_library_prompt", { request });
}

export async function saveLibraryItem(
  request: SaveLibraryItemRequest,
  sessionId?: string | null,
): Promise<SavedLibraryItem> {
  return invoke("save_library_item", { request, sessionId: sessionId ?? null });
}

export async function getLibraryPinnedRepos(): Promise<string[]> {
  return invoke("get_library_pinned_repos");
}

export async function setLibraryPinnedRepos(pinnedRepos: string[]): Promise<string[]> {
  return invoke("set_library_pinned_repos", { pinnedRepos });
}

export async function listLibrarySources(): Promise<LibrarySource[]> {
  return invoke("list_library_sources");
}

export async function setLibrarySources(sources: LibrarySource[]): Promise<LibrarySource[]> {
  return invoke("set_library_sources", { sources });
}

export async function cloneLibrarySource(sourceId: string): Promise<string> {
  return invoke("clone_library_source", { sourceId });
}

export async function syncLibrarySource(sourceId: string): Promise<LibraryGitStatus> {
  return invoke("sync_library_source", { sourceId });
}

export async function getLibrarySourceStatus(sourceId: string): Promise<LibraryGitStatus> {
  return invoke("get_library_source_status", { sourceId });
}

export async function getLibrarySourceStatuses(): Promise<LibraryGitStatus[]> {
  return invoke("get_library_source_statuses");
}

// Library skill sync

export type SkillSyncOutcomeKind =
  | "synced"
  | "syncedAsCopyFallback"
  | "skipped"
  | "failed";

export type SkillSyncSkipReason = "alreadyUpToDate" | "untrackedFile" | "userEdited";

export interface SkillSyncResult {
  skillId: string;
  sourceId?: string | null;
  destination: string;
  requestedMode: SkillSyncMode;
  outcome: SkillSyncOutcomeKind;
  skipReason?: SkillSyncSkipReason | null;
  error?: string | null;
}

export interface SkillSyncEntry {
  skillId: string;
  sourceId?: string | null;
  destination: string;
  mode: SkillSyncMode;
  syncedAt: string;
}

export interface SkillSyncRunReport {
  results: SkillSyncResult[];
  /** Manifest entries no longer in the desired set (skill removed, source disabled). */
  stale: SkillSyncEntry[];
  /** How many syncs auto-degraded from symlink to copy (Windows OS denial). */
  symlinkFallbackCount: number;
}

export type UnsyncScope =
  | { type: "all" }
  | { type: "stale"; value: string[] }
  | { type: "source"; value: string };

export type UnsyncOutcomeKind = "deleted" | "keptDueToDrift" | "alreadyGone" | "failed";

export interface UnsyncResult {
  skillId: string;
  sourceId?: string | null;
  destination: string;
  outcome: UnsyncOutcomeKind;
  error?: string | null;
}

export interface UnsyncReport {
  results: UnsyncResult[];
}

export async function librarySkillSyncRun(
  sessionId?: string | null,
): Promise<SkillSyncRunReport> {
  return invoke("library_skill_sync_run", { sessionId: sessionId ?? null });
}

export async function librarySkillSyncUnsync(scope: UnsyncScope): Promise<UnsyncReport> {
  return invoke("library_skill_sync_unsync", { scope });
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
import type {
  AgentNotificationSetupStatus,
  CodexNotificationConfigPreview,
  SetupStatus,
} from "$lib/bindings";
export type {
  AgentNotificationSetupStatus,
  CodexNotificationConfigPreview,
  SetupStatus,
};

export async function checkSetupNeeded(): Promise<boolean> {
  return invoke("check_setup_needed");
}

export async function checkSetupStatus(): Promise<SetupStatus> {
  return invoke("check_setup_status");
}

export async function agentNotificationSetupStatus(): Promise<AgentNotificationSetupStatus> {
  return invoke("cmd_agent_notification_setup_status");
}

export async function previewCodexNotificationConfig(): Promise<CodexNotificationConfigPreview> {
  return invoke("cmd_preview_codex_notification_config");
}

export async function configureCodexNotificationConfig(): Promise<void> {
  return invoke("cmd_configure_codex_notification_config");
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
export type PrChecksState = "passing" | "failing" | "pending" | "none";
export type PrCheckStatus = "passing" | "failing" | "pending";

export interface PrChecksSummary {
  state: PrChecksState;
  passing: number;
  failing: number;
  pending: number;
  total: number;
}

export interface PrCheckDetails {
  name: string;
  status: PrCheckStatus;
  url: string | null;
}

export interface PrReviewDetails {
  reviewer: string;
  state: string;
  url: string | null;
}

export interface PrInfo {
  number: number;
  title: string;
  headRef: string;
  headOwner: string;
  isCrossRepository: boolean;
  url: string;
  repoSlug: string;
  checks: PrChecksSummary | null;
  checkRuns: PrCheckDetails[];
  /** GitHub's `reviewDecision`: "APPROVED" | "CHANGES_REQUESTED" |
   *  "REVIEW_REQUIRED", or null when there's no decision yet. */
  reviewDecision: string | null;
  reviewDetails: PrReviewDetails[];
}

export async function checkGhInstalled(): Promise<boolean> {
  return invoke("check_gh_installed");
}

export async function lookupPr(repoPath: string | null, url: string): Promise<PrInfo> {
  return invoke("lookup_pr", { repoPath, url });
}

export async function lookupPrForBranch(
  repoPath: string,
  branch: string,
): Promise<PrInfo | null> {
  return invoke("lookup_pr_for_branch", { repoPath, branch });
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

export async function updateProject(
  id: string,
  patch: ProjectUpdate,
): Promise<Project> {
  return invoke("update_project", { id, patch });
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

export async function setSessionPinnedPrUrl(
  sessionId: string,
  url: string | null,
): Promise<void> {
  return invoke("set_session_pinned_pr_url", { sessionId, url });
}

export async function setSessionSmolMachine(
  sessionId: string,
  machineName: string | null,
): Promise<void> {
  return invoke("set_session_smol_machine", { sessionId, machineName });
}

export async function refreshSessionBranch(
  sessionId: string,
): Promise<string | null> {
  return invoke("refresh_session_branch", { sessionId });
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
  /** Last human prompt extracted from Claude's transcript on Stop, when available. */
  query: string | null;
  /** Last assistant response extracted from Claude's transcript on Stop, when available. */
  response: string | null;
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

export async function findOrCreateWatch(config: CreateWatchConfig): Promise<Watch> {
  return invoke("cmd_find_or_create_watch", { config });
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

export async function listAllPtys(): Promise<import("./bindings").PtyInfo[]> {
  return invoke("list_all_ptys");
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
  provider?: "claude" | "codex";
  providerSessionId?: string;
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

// ── Mailbox + alias events ────────────────────────────────────────────────────
//
// Hand-rolled command wrappers because `Event.structured` is `serde_json::Value`,
// which specta cannot render as TypeScript. Mirrors how `pane_state` is exposed.

import type {
  AgentAlias,
  AliasEvent,
  BusSubscription,
  BusSubscriptionEvent,
  ConsumptionMode,
  Event as MailboxEventPayload,
  MailboxEvent,
  ReadState,
} from "./types/mailbox";

export type {
  AgentAlias,
  AliasEvent,
  AliasMember,
  BusSubscription,
  BusSubscriptionEvent,
  ConsumptionMode,
  EventKind,
  MailboxEvent,
  ReadState,
} from "./types/mailbox";
export type { Event as MailboxEventPayload } from "./types/mailbox";

export function onMailboxEvent(
  callback: (payload: MailboxEvent) => void,
): Promise<UnlistenFn> {
  return listen<MailboxEvent>("mailbox-event", (e) => callback(e.payload));
}

export function onAliasEvent(
  callback: (payload: AliasEvent) => void,
): Promise<UnlistenFn> {
  return listen<AliasEvent>("alias-event", (e) => callback(e.payload));
}

export interface MailboxListOptions {
  unreadOnly?: boolean;
  projectId?: string | null;
  global?: boolean;
}

export async function mailboxListForRecipient(
  alias: string,
  options: MailboxListOptions = {},
): Promise<MailboxEventPayload[]> {
  return invoke("mailbox_list_for_recipient", {
    alias,
    unreadOnly: options.unreadOnly ?? null,
    projectId: options.projectId ?? null,
    global: options.global ?? null,
  });
}

export async function mailboxListForTopic(
  topic: string,
  options: { projectId?: string | null; global?: boolean } = {},
): Promise<MailboxEventPayload[]> {
  return invoke("mailbox_list_for_topic", {
    topic,
    projectId: options.projectId ?? null,
    global: options.global ?? null,
  });
}

export async function mailboxListAll(
  options: { projectId?: string | null; global?: boolean; limit?: number } = {},
): Promise<MailboxEventPayload[]> {
  return invoke("mailbox_list_all", {
    projectId: options.projectId ?? null,
    global: options.global ?? null,
    limit: options.limit ?? null,
  });
}

export async function mailboxUnreadCount(
  alias: string,
  options: { projectId?: string | null; global?: boolean } = {},
): Promise<number> {
  return invoke("mailbox_unread_count", {
    alias,
    projectId: options.projectId ?? null,
    global: options.global ?? null,
  });
}

export async function mailboxGetEvent(
  eventId: string,
): Promise<MailboxEventPayload | null> {
  return invoke("mailbox_get_event", { eventId });
}

export async function mailboxReadState(
  eventId: string,
  recipient: string,
): Promise<ReadState | null> {
  return invoke("mailbox_read_state", { eventId, recipient });
}

export interface MailboxPostInput {
  to?: string | null;
  topic?: string | null;
  body: string;
  subject?: string | null;
  kind?: import("./types/mailbox").EventKind | null;
  projectId?: string | null;
  correlationId?: string | null;
  structured?: unknown;
  from?: string | null;
}

export async function mailboxPost(
  input: MailboxPostInput,
): Promise<MailboxEventPayload> {
  return invoke("mailbox_post", {
    input: {
      to: input.to ?? null,
      topic: input.topic ?? null,
      body: input.body,
      subject: input.subject ?? null,
      kind: input.kind ?? null,
      projectId: input.projectId ?? null,
      correlationId: input.correlationId ?? null,
      structured: input.structured ?? null,
      from: input.from ?? null,
    },
  });
}

export async function mailboxMarkRead(
  eventId: string,
  recipient: string,
): Promise<boolean> {
  return invoke("mailbox_mark_read", { eventId, recipient });
}

export async function mailboxAck(
  eventId: string,
  recipient: string,
  result: string | null = null,
): Promise<boolean> {
  return invoke("mailbox_ack", { eventId, recipient, result });
}

export async function mailboxClearRead(recipient: string): Promise<number> {
  return invoke("mailbox_clear_read", { recipient });
}

/**
 * Sender-side unsend. Sets retracted_at on the event so recipients drop
 * it from their inbox views. Allowed only when no recipient has acked
 * yet — once anyone confirmed delivery the audit trail is preserved.
 */
export async function mailboxRetract(
  eventId: string,
  sender: string,
): Promise<MailboxEventPayload> {
  return invoke("mailbox_retract", { eventId, sender });
}

/**
 * Recipient-side single-event hide. Sets cleared_at on the
 * (eventId, recipient) ReadState regardless of read state. The event
 * itself is preserved; only this recipient's view loses it.
 */
export async function mailboxDismiss(
  eventId: string,
  recipient: string,
): Promise<boolean> {
  return invoke("mailbox_dismiss", { eventId, recipient });
}

/**
 * Type a mailbox event's body into the recipient's pane (plus trailing CR
 * so Claude/Codex see it as submitted input). Backend looks up
 * recipient → alias → pane_id → pty_id and writes via the existing PTY
 * write path. Auto-acks the event with a "delivered" marker.
 *
 * Errors when the recipient alias has no pane bound — that's the
 * "delivery requires a live pane" rule. Use `roux mailbox post` for
 * durable queueing without delivery.
 */
export async function mailboxDeliverToPane(eventId: string): Promise<void> {
  return invoke("mailbox_deliver_to_pane", { eventId });
}

export async function aliasesList(
  options: {
    projectId?: string | null;
    global?: boolean;
    onlyUnbound?: boolean;
  } = {},
): Promise<AgentAlias[]> {
  return invoke("aliases_list", {
    projectId: options.projectId ?? null,
    global: options.global ?? null,
    onlyUnbound: options.onlyUnbound ?? null,
  });
}

export async function aliasesGet(
  alias: string,
  projectId: string | null = null,
): Promise<AgentAlias | null> {
  return invoke("aliases_get", { alias, projectId });
}

export async function aliasesWhoami(sessionId: string): Promise<AgentAlias[]> {
  return invoke("aliases_whoami", { sessionId });
}

export async function aliasesAddMember(
  alias: string,
  paneId: string,
  projectId: string | null = null,
): Promise<AgentAlias> {
  return invoke("aliases_add_member", { alias, paneId, projectId });
}

export async function aliasesRemoveMember(
  alias: string,
  paneId: string,
  projectId: string | null = null,
): Promise<boolean> {
  return invoke("aliases_remove_member", { alias, paneId, projectId });
}

export async function aliasesSetMode(
  alias: string,
  mode: ConsumptionMode,
  projectId: string | null = null,
): Promise<AgentAlias> {
  return invoke("aliases_set_mode", { alias, mode, projectId });
}

// ── Bus subscriptions ─────────────────────────────────────────────────────────

export function onBusSubscriptionEvent(
  callback: (payload: BusSubscriptionEvent) => void,
): Promise<UnlistenFn> {
  return listen<BusSubscriptionEvent>("subscription-event", (e) =>
    callback(e.payload),
  );
}

export async function subscriptionsList(
  options: {
    alias?: string | null;
    projectId?: string | null;
    global?: boolean;
  } = {},
): Promise<BusSubscription[]> {
  return invoke("subscriptions_list", {
    alias: options.alias ?? null,
    projectId: options.projectId ?? null,
    global: options.global ?? null,
  });
}

export async function subscriptionsCreate(
  alias: string,
  pattern: string,
  projectId: string | null = null,
): Promise<BusSubscription> {
  return invoke("subscriptions_create", { alias, pattern, projectId });
}

export async function subscriptionsDelete(id: string): Promise<boolean> {
  return invoke("subscriptions_delete", { id });
}
