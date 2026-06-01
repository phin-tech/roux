<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";
  import {
    sidebarLayout,
    setRailSide,
    type Side,
  } from "$lib/stores/sidebarLayout";
  import {
    settingsModalSize,
    setSettingsModalSize,
    SETTINGS_MODAL_MAX_WIDTH,
    SETTINGS_MODAL_MAX_HEIGHT,
  } from "$lib/stores/settingsModalSize";
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { THEME_DEFINITIONS, getAllTerminalThemeDefinitions } from "$lib/themes";
  import { userTerminalThemes, loadUserTerminalThemes } from "$lib/stores/userTerminalThemes";
  import { profileList, type SpawnProfile } from "$lib/panes/profiles";
  import { getLogPath, setLoggingEnabled } from "$lib/logging";
  import {
    getRuntimeStatus,
    notificationsPush,
    quitApp,
    type RuntimeStatus,
  } from "$lib/tauri";
  import { commands } from "$lib/bindings";
  import type {
    AgentNotificationProviderStatus,
    AgentNotificationSetupStatus,
    CodexNotificationConfigPreview,
    GpuAcceleration,
    ExternalTool,
    ExternalToolSurface,
    ExternalToolWebEmbedder,
    IntegrationDetection,
    KanbanSettings,
    KanbanStartupSidebar,
    McpHostConfigPreview,
    McpStatus,
    OnPaneCloseMode,
    UpdateChannel,
    WorktreeCleanupMode,
    WorktreeDefaultBase,
    WorktreeProvider,
    WorktrunkDetection,
  } from "$lib/bindings";
  import { updateStatus, runManualCheck, performInstall } from "$lib/stores/updater";
  import { activeSession } from "$lib/stores/sessions";
  import { settingsFocus } from "$lib/stores/settingsFocus";
  import { previewExternalToolConfig, type RenderedExternalTool } from "$lib/tauri";
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import Settings from "@lucide/svelte/icons/settings";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import TerminalIcon from "@lucide/svelte/icons/terminal";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Bell from "@lucide/svelte/icons/bell";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Wrench from "@lucide/svelte/icons/wrench";
  import Plug from "@lucide/svelte/icons/plug";
  import NotebookPen from "@lucide/svelte/icons/notebook-pen";
  import ClipboardList from "@lucide/svelte/icons/clipboard-list";
  import FlaskConical from "@lucide/svelte/icons/flask-conical";
  import X from "@lucide/svelte/icons/x";
  import DoctorPanel from "$lib/components/DoctorPanel.svelte";
  import {
    EXPERIMENTS,
    currentExperimentValue,
    withExperimentValue,
  } from "$lib/experiments";

  type CategoryId = "general" | "sessions" | "terminal" | "kanban" | "claude" | "notes" | "integrations" | "notifications" | "keyboard" | "experiments" | "advanced";

  const CATEGORIES: { id: CategoryId; label: string; icon: typeof Settings }[] = [
    { id: "general", label: "General", icon: Settings },
    { id: "sessions", label: "Sessions", icon: FolderTree },
    { id: "terminal", label: "Terminal", icon: TerminalIcon },
    { id: "kanban", label: "Kanban", icon: ClipboardList },
    { id: "claude", label: "Claude", icon: Sparkles },
    { id: "notes", label: "Notes", icon: NotebookPen },
    { id: "integrations", label: "Integrations", icon: Plug },
    { id: "notifications", label: "Notifications", icon: Bell },
    { id: "keyboard", label: "Keyboard", icon: Keyboard },
    { id: "experiments", label: "Experiments", icon: FlaskConical },
    { id: "advanced", label: "Advanced", icon: Wrench },
  ];

  const PANE_CLOSE_OPTIONS = [
    { id: "kill", label: "Kill" },
    { id: "detach", label: "Detach" },
  ] as const;

  const KANBAN_DEFAULTS: KanbanSettings = {
    defaultAgentProfile: "claude",
    planningPromptAppend: "",
    implementationPromptAppend: "",
    reviewPromptAppend: "",
    startupSidebar: "restore",
  };

  const autonomousProfiles = $derived(
    $profileList.filter((profile: SpawnProfile) => {
      const provider = profile.provider ?? null;
      const command = profile.startupCommand?.trim() ?? "";
      return (
        (provider === "claude" || provider === "codex") &&
        profile.startupBehavior !== "typeOnly" &&
        command.length > 0
      );
    }),
  );

  let selected = $state<CategoryId>("general");

  let appVersion = $state<string>("…");
  let runtimeStatus = $state<RuntimeStatus | null>(null);
  let runtimeStatusError = $state<string | null>(null);
  let runtimeStatusLoading = $state(false);
  let repoRootDraft = $state("");
  let expandedExternalToolId = $state<string | null>(null);
  let externalToolPreviewById = $state<
    Record<string, { loading: boolean; rendered: RenderedExternalTool | null; error: string | null }>
  >({});
  const externalToolRowKeys = new Map<string, string>();
  let nextExternalToolRowKey = 0;
  let wasVisible = false;
  onMount(async () => {
    try { appVersion = await getVersion(); } catch { appVersion = "unknown"; }
  });

  function describeError(reason: "network" | "signature-invalid" | "unknown"): string {
    switch (reason) {
      case "network": return "Couldn't reach the update server.";
      case "signature-invalid": return "Update signature invalid. Download blocked.";
      case "unknown": return "Update check failed.";
    }
  }

  let notifTestStatus = $state<"idle" | "sent" | "error">("idle");
  let notifTestError = $state<string | null>(null);

  async function sendTestNotification() {
    notifTestStatus = "idle";
    notifTestError = null;
    try {
      await notificationsPush({
        level: "attention",
        source: { type: "cli" },
        title: "Roux notification test",
        subtitle: null,
        body: "If you saw a macOS notification, permissions are set up correctly.",
        sessionId: null,
        actions: [],
        dedupKey: null,
      });
      notifTestStatus = "sent";
    } catch (e) {
      notifTestStatus = "error";
      notifTestError = e instanceof Error ? e.message : String(e);
    }
  }

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  $effect(() => {
    const justOpened = visible && !wasVisible;
    wasVisible = visible;
    if (!visible) return;
    const focus = $settingsFocus;
    if (focus?.category) {
      selected = focus.category as CategoryId;
      if (focus.category === "integrations" && "externalToolId" in focus) {
        expandedExternalToolId = focus.externalToolId ?? null;
      }
      settingsFocus.set(null);
      return;
    }
    if (justOpened) selected = "general";
  });

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  // Drag-to-resize. The modal stays centered, so each edge moves by half the
  // size delta; doubling the cursor delta keeps the bottom-right corner under
  // the pointer. Persisted size is clamped to absolute bounds by the store and
  // to the live viewport here so it can't grow off-screen.
  let resizing = $state(false);
  let resizeStart = { x: 0, y: 0, w: 0, h: 0 };

  function onResizePointerDown(e: PointerEvent) {
    e.preventDefault();
    e.stopPropagation();
    resizing = true;
    resizeStart = {
      x: e.clientX,
      y: e.clientY,
      w: $settingsModalSize.width,
      h: $settingsModalSize.height,
    };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onResizePointerMove(e: PointerEvent) {
    if (!resizing) return;
    const maxW = Math.min(SETTINGS_MODAL_MAX_WIDTH, window.innerWidth - 32);
    const maxH = Math.min(SETTINGS_MODAL_MAX_HEIGHT, window.innerHeight - 32);
    const w = Math.min(resizeStart.w + (e.clientX - resizeStart.x) * 2, maxW);
    const h = Math.min(resizeStart.h + (e.clientY - resizeStart.y) * 2, maxH);
    setSettingsModalSize(w, h);
  }

  function externalTools(): ExternalTool[] {
    return $settings.externalTools ?? [];
  }

  function externalToolRowKey(id: string): string {
    let key = externalToolRowKeys.get(id);
    if (!key) {
      key = `external-tool-row-${++nextExternalToolRowKey}`;
      externalToolRowKeys.set(id, key);
    }
    return key;
  }

  function retainExternalToolRowKey(previousId: string, nextId: string): void {
    if (previousId === nextId) return;
    const key = externalToolRowKeys.get(previousId);
    if (!key) return;
    externalToolRowKeys.delete(previousId);
    externalToolRowKeys.set(nextId, key);
  }

  function pruneExternalToolRowKeys(tools: ExternalTool[]): void {
    const ids = new Set(tools.map((tool) => tool.id));
    for (const id of externalToolRowKeys.keys()) {
      if (!ids.has(id)) externalToolRowKeys.delete(id);
    }
  }

  function updateExternalTools(tools: ExternalTool[]): void {
    pruneExternalToolRowKeys(tools);
    updateSetting("externalTools", tools);
  }

  function updateExternalTool(id: string, patch: Partial<ExternalTool>): void {
    const tools = externalTools();
    let nextPatch = patch;
    if (patch.id !== undefined) {
      const normalizedId = patch.id.trim();
      if (!normalizedId || tools.some((tool) => tool.id !== id && tool.id.trim() === normalizedId)) {
        return;
      }
      nextPatch = { ...patch, id: normalizedId };
    }
    if (nextPatch.id !== undefined) {
      retainExternalToolRowKey(id, nextPatch.id);
      if (expandedExternalToolId === id) expandedExternalToolId = nextPatch.id;
    }
    updateExternalTools(tools.map((tool) => (tool.id === id ? { ...tool, ...nextPatch } : tool)));
  }

  function preferredPortFromInput(value: string): number | null {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number.parseInt(trimmed, 10);
    if (Number.isNaN(parsed)) return null;
    return Math.min(65535, Math.max(1, parsed));
  }

  function addExternalTool(surface: ExternalToolSurface): void {
    const id = `tool-${Date.now()}`;
    const tool: ExternalTool = {
      id,
      name: surface === "web" ? "New Web Tool" : "New Terminal Tool",
      enabled: true,
      surface,
      commandTemplate: surface === "web" ? "server --port {{ port }}" : "command",
      cwdTemplate: "{{ session.worktree_path }}",
      requiresSession: true,
      urlTemplate: surface === "web" ? "http://127.0.0.1:{{ port }}" : null,
      preferredPort: surface === "web" ? 4966 : null,
      webEmbedder: "webview",
    };
    updateExternalTools([...externalTools(), tool]);
    expandedExternalToolId = id;
  }

  function removeExternalTool(id: string): void {
    updateExternalTools(externalTools().filter((tool) => tool.id !== id));
    if (expandedExternalToolId === id) expandedExternalToolId = null;
  }

  async function previewTool(tool: ExternalTool): Promise<void> {
    externalToolPreviewById = {
      ...externalToolPreviewById,
      [tool.id]: { loading: true, rendered: null, error: null },
    };
    try {
      const rendered = await previewExternalToolConfig(
        tool,
        tool.requiresSession ? ($activeSession?.id ?? null) : null,
        tool.surface === "web" ? (tool.preferredPort ?? 4966) : null,
      );
      externalToolPreviewById = {
        ...externalToolPreviewById,
        [tool.id]: { loading: false, rendered, error: null },
      };
    } catch (err) {
      externalToolPreviewById = {
        ...externalToolPreviewById,
        [tool.id]: {
          loading: false,
          rendered: null,
          error: err instanceof Error ? err.message : String(err),
        },
      };
    }
  }

  function onResizePointerUp(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {}
  }

  async function browseClaudeBinary() {
    const selected = await open({ directory: false, title: "Select Claude Binary" });
    if (selected) updateSetting("claudeBinaryPath", selected as string);
  }

  async function browseGhBinary() {
    const selected = await open({ directory: false, title: "Select gh (GitHub CLI) Binary" });
    if (selected) updateSetting("ghBinaryPath", selected as string);
  }

  async function browseGitBinary() {
    const selected = await open({ directory: false, title: "Select git Binary" });
    if (selected) updateSetting("gitBinaryPath", selected as string);
  }

  async function browseWorktrunkBinary() {
    const selected = await open({
      directory: false,
      title: "Select wt (worktrunk) Binary",
    });
    if (selected) updateSetting("worktrunkBinaryPath", selected as string);
  }

  async function browseShellBinary() {
    const selected = await open({
      directory: false,
      title: "Select Shell Binary",
    });
    if (selected) updateSetting("shellBinaryPath", selected as string);
  }

  function kanbanSettings(): KanbanSettings {
    return { ...KANBAN_DEFAULTS, ...($settings.kanban ?? {}) };
  }

  function updateKanban<K extends keyof KanbanSettings>(
    key: K,
    value: KanbanSettings[K],
  ): void {
    updateSetting("kanban", { ...kanbanSettings(), [key]: value });
  }

  let ghDetection = $state<IntegrationDetection | null>(null);
  let gitDetection = $state<IntegrationDetection | null>(null);
  let worktrunkDetection = $state<WorktrunkDetection | null>(null);
  let mcpStatus = $state<McpStatus | null>(null);
  // Per-host UI state, keyed by host id (`claudeDesktop`, `claudeCode`,
  // `codex`). Each host gets its own preview blob + status message so a
  // configure-Codex run doesn't blow away a preview-Claude-Code result.
  let mcpPreviewByHost = $state<Record<string, McpHostConfigPreview | null>>({});
  let mcpMessageByHost = $state<Record<string, string | null>>({});
  let mcpErrorByHost = $state<Record<string, string | null>>({});
  let mcpBusyByHost = $state<Record<string, "preview" | "configure" | null>>({});
  let agentNotificationStatus = $state<AgentNotificationSetupStatus | null>(null);
  let agentNotificationMessage = $state<string | null>(null);
  let agentNotificationError = $state<string | null>(null);
  let codexNotificationPreview = $state<CodexNotificationConfigPreview | null>(null);
  let agentNotificationBusy = $state<"refresh" | "claude" | "codex-preview" | "codex-configure" | null>(null);
  const claudeNotificationProvider = $derived(
    agentNotificationStatus?.providers.find((provider) => provider.provider === "claude") ?? null,
  );
  const codexNotificationProvider = $derived(
    agentNotificationStatus?.providers.find((provider) => provider.provider === "codex") ?? null,
  );
  let ghDetectionRun = 0;
  let gitDetectionRun = 0;
  let worktrunkDetectionRun = 0;
  let mcpStatusRun = 0;
  let agentNotificationStatusRun = 0;
  let runtimeStatusRun = 0;

  async function refreshGhDetection(run: number) {
    try {
      const result = await commands.cmdDetectGh();
      if (run === ghDetectionRun) ghDetection = result;
    } catch {
      if (run === ghDetectionRun) ghDetection = { binaryPath: null, version: null };
    }
  }

  async function refreshGitDetection(run: number) {
    try {
      const result = await commands.cmdDetectGit();
      if (run === gitDetectionRun) gitDetection = result;
    } catch {
      if (run === gitDetectionRun) gitDetection = { binaryPath: null, version: null };
    }
  }

  async function refreshWorktrunkDetection(run: number) {
    try {
      const result = await commands.cmdDetectWorktrunk(null);
      if (run === worktrunkDetectionRun) worktrunkDetection = result;
    } catch {
      if (run === worktrunkDetectionRun) {
        worktrunkDetection = { binaryPath: null, version: null, hasConfig: false };
      }
    }
  }

  async function refreshMcpStatus(run: number) {
    try {
      const result = await commands.cmdMcpStatus();
      if (run === mcpStatusRun) mcpStatus = result;
    } catch {
      if (run === mcpStatusRun) mcpStatus = null;
    }
  }

  async function refreshAgentNotificationStatus(run: number) {
    try {
      const result = await commands.cmdAgentNotificationSetupStatus();
      if (run === agentNotificationStatusRun) {
        agentNotificationStatus = result;
        agentNotificationError = null;
      }
    } catch (e) {
      if (run === agentNotificationStatusRun) {
        agentNotificationStatus = null;
        agentNotificationError = e instanceof Error ? e.message : String(e);
      }
    }
  }

  async function refreshRuntimeStatus(run: number) {
    runtimeStatusLoading = true;
    runtimeStatusError = null;
    try {
      const result = await getRuntimeStatus();
      if (run === runtimeStatusRun) runtimeStatus = result;
    } catch (e) {
      if (run === runtimeStatusRun) {
        runtimeStatus = null;
        runtimeStatusError = e instanceof Error ? e.message : String(e);
      }
    } finally {
      if (run === runtimeStatusRun) runtimeStatusLoading = false;
    }
  }

  $effect(() => {
    const path = $settings.ghBinaryPath;
    void path;
    if (!visible || selected !== "integrations") {
      ghDetectionRun += 1;
      return;
    }
    const run = ++ghDetectionRun;
    const timer = setTimeout(() => void refreshGhDetection(run), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const path = $settings.gitBinaryPath;
    void path;
    if (!visible || selected !== "integrations") {
      gitDetectionRun += 1;
      return;
    }
    const run = ++gitDetectionRun;
    const timer = setTimeout(() => void refreshGitDetection(run), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const path = $settings.worktrunkBinaryPath;
    void path;
    if (!visible || selected !== "integrations") {
      worktrunkDetectionRun += 1;
      return;
    }
    const run = ++worktrunkDetectionRun;
    const timer = setTimeout(() => void refreshWorktrunkDetection(run), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const enabled = $settings.mcpEnabled;
    void enabled;
    if (!visible || selected !== "integrations") {
      mcpStatusRun += 1;
      return;
    }
    const run = ++mcpStatusRun;
    const timer = setTimeout(() => void refreshMcpStatus(run), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!visible || selected !== "notifications") {
      agentNotificationStatusRun += 1;
      return;
    }
    const run = ++agentNotificationStatusRun;
    const timer = setTimeout(() => void refreshAgentNotificationStatus(run), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!visible || selected !== "advanced") {
      runtimeStatusRun += 1;
      return;
    }
    const run = ++runtimeStatusRun;
    void refreshRuntimeStatus(run);
  });

  function runtimeModeLabel(status: RuntimeStatus): string {
    return status.mode === "daemon" ? "Daemon" : "Local fallback";
  }

  function formatTimestamp(ms: number): string {
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) return String(ms);
    return date.toLocaleString();
  }

  function formatDuration(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000));
    const days = Math.floor(totalSeconds / 86_400);
    const hours = Math.floor((totalSeconds % 86_400) / 3_600);
    const minutes = Math.floor((totalSeconds % 3_600) / 60);
    const seconds = totalSeconds % 60;
    if (days > 0) return `${days}d ${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h ${minutes}m`;
    if (minutes > 0) return `${minutes}m ${seconds}s`;
    return `${seconds}s`;
  }

  type McpHostIdT = "claudeDesktop" | "claudeCode" | "codex";

  async function previewMcpHostConfig(hostId: McpHostIdT, label: string) {
    mcpBusyByHost = { ...mcpBusyByHost, [hostId]: "preview" };
    mcpErrorByHost = { ...mcpErrorByHost, [hostId]: null };
    mcpMessageByHost = { ...mcpMessageByHost, [hostId]: null };
    try {
      const result = await commands.cmdPreviewMcpHostConfig(hostId);
      if (result.status === "error") {
        throw new Error(result.error);
      }
      mcpPreviewByHost = { ...mcpPreviewByHost, [hostId]: result.data };
      mcpMessageByHost = {
        ...mcpMessageByHost,
        [hostId]: result.data.configured
          ? `${label} is already configured.`
          : "Preview ready.",
      };
    } catch (e) {
      mcpPreviewByHost = { ...mcpPreviewByHost, [hostId]: null };
      mcpErrorByHost = {
        ...mcpErrorByHost,
        [hostId]: e instanceof Error ? e.message : String(e),
      };
    } finally {
      mcpBusyByHost = { ...mcpBusyByHost, [hostId]: null };
    }
  }

  async function configureMcpHost(hostId: McpHostIdT, label: string) {
    mcpBusyByHost = { ...mcpBusyByHost, [hostId]: "configure" };
    mcpErrorByHost = { ...mcpErrorByHost, [hostId]: null };
    mcpMessageByHost = { ...mcpMessageByHost, [hostId]: null };
    try {
      const result = await commands.cmdConfigureMcpHost(hostId);
      if (result.status === "error") {
        throw new Error(result.error);
      }
      mcpPreviewByHost = { ...mcpPreviewByHost, [hostId]: result.data };
      mcpMessageByHost = {
        ...mcpMessageByHost,
        [hostId]: result.data.configured
          ? `${label} was already configured.`
          : `${label} configuration updated.`,
      };
      const run = ++mcpStatusRun;
      await refreshMcpStatus(run);
    } catch (e) {
      mcpErrorByHost = {
        ...mcpErrorByHost,
        [hostId]: e instanceof Error ? e.message : String(e),
      };
    } finally {
      mcpBusyByHost = { ...mcpBusyByHost, [hostId]: null };
    }
  }

  async function configureClaudeNotifications() {
    agentNotificationBusy = "claude";
    agentNotificationError = null;
    agentNotificationMessage = null;
    try {
      const result = await commands.reinstallHooks();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      agentNotificationMessage = "Claude Code hooks installed.";
      const run = ++agentNotificationStatusRun;
      await refreshAgentNotificationStatus(run);
    } catch (e) {
      agentNotificationError = e instanceof Error ? e.message : String(e);
    } finally {
      agentNotificationBusy = null;
    }
  }

  async function previewCodexNotifications() {
    agentNotificationBusy = "codex-preview";
    agentNotificationError = null;
    agentNotificationMessage = null;
    try {
      const result = await commands.cmdPreviewCodexNotificationConfig();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      codexNotificationPreview = result.data;
      agentNotificationMessage = codexNotificationPreview.configured
        ? "Codex notifications are already configured."
        : "Codex config preview ready.";
    } catch (e) {
      codexNotificationPreview = null;
      agentNotificationError = e instanceof Error ? e.message : String(e);
    } finally {
      agentNotificationBusy = null;
    }
  }

  async function configureCodexNotifications() {
    agentNotificationBusy = "codex-configure";
    agentNotificationError = null;
    agentNotificationMessage = null;
    try {
      const result = await commands.cmdConfigureCodexNotificationConfig();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      agentNotificationMessage = "Codex notifications configured.";
      const preview = await commands.cmdPreviewCodexNotificationConfig();
      if (preview.status === "ok") {
        codexNotificationPreview = preview.data;
      }
      const run = ++agentNotificationStatusRun;
      await refreshAgentNotificationStatus(run);
    } catch (e) {
      agentNotificationError = e instanceof Error ? e.message : String(e);
    } finally {
      agentNotificationBusy = null;
    }
  }

  function notificationProviderLabel(provider: AgentNotificationProviderStatus | null): string {
    if (!provider) return "checking";
    switch (provider.status) {
      case "installed":
        return "configured";
      case "missing":
        return "not configured";
      case "stale":
        return "needs update";
      case "error":
        return "error";
      case "unavailable":
        return "unavailable";
      default:
        return provider.status;
    }
  }

  function notificationProviderClass(provider: AgentNotificationProviderStatus | null): string {
    const status = provider?.status ?? "checking";
    switch (status) {
      case "installed":
        return "rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green";
      case "stale":
        return "rounded bg-amber/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-amber";
      case "error":
        return "rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red";
      default:
        return "rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted";
    }
  }

  function formatMcpConfiguredAt(ms: number | null | undefined): string {
    if (!ms) return "Never";
    return new Date(ms).toLocaleString();
  }

  async function browseWorktreeBase() {
    const selected = await open({ directory: true, title: "Select Worktree Base Directory" });
    if (selected) updateSetting("worktreeBasePath", selected as string);
  }

  async function browseDefaultProject() {
    const selected = await open({ directory: true, title: "Select Default Project Directory" });
    if (selected) updateSetting("defaultProjectPath", selected as string);
  }

  function sanitizePathInput(path: string): string {
    return path.trim();
  }

  function addRepoRoot(path: string) {
    const nextPath = sanitizePathInput(path);
    if (!nextPath) return;
    const existing = $settings.repoRoots ?? [];
    if (existing.includes(nextPath)) {
      repoRootDraft = "";
      return;
    }
    updateSetting("repoRoots", [...existing, nextPath]);
    repoRootDraft = "";
  }

  function removeRepoRoot(path: string) {
    updateSetting(
      "repoRoots",
      ($settings.repoRoots ?? []).filter((root) => root !== path),
    );
  }

  // Pick a repo path to use for the Settings worktree-base-path preview.
  // Falls back to an illustrative placeholder when the user hasn't
  // configured any repo roots or a default project yet — the preview is
  // purely informational, so a made-up path is fine.
  const PREVIEW_FALLBACK = "/Users/you/src/my-project";
  let previewText = $state<string>("");
  function previewRepoPath(): string {
    const roots = $settings.repoRoots ?? [];
    if (roots.length > 0) return roots[0];
    if ($settings.defaultProjectPath) return $settings.defaultProjectPath;
    return PREVIEW_FALLBACK;
  }
  $effect(() => {
    const tpl = $settings.worktreeBasePath ?? "";
    const repo = previewRepoPath();
    // Cancel-on-stale guard: only keep the latest preview result.
    let stale = false;
    commands
      .cmdPreviewWorktreeBase(tpl, repo)
      .then((r) => { if (!stale) previewText = r; })
      .catch(() => { if (!stale) previewText = ""; });
    return () => { stale = true; };
  });

  function setCleanupMode(mode: WorktreeCleanupMode) {
    updateSetting("worktreeCleanupOnClose", mode);
    // Keep the legacy boolean in sync so any older readers (settings files,
    // pre-migration code) still agree with the frontend.
    updateSetting("cleanupWorktreesOnClose", mode === "always");
  }

  function setDefaultBase(mode: WorktreeDefaultBase) {
    updateSetting("worktreeDefaultBase", mode);
  }

  function setPaneCloseMode(mode: OnPaneCloseMode) {
    updateSetting("onPaneClose", mode);
  }

  async function browseAndAddRepoRoot() {
    const selected = await open({ directory: true, title: "Select Repo Root Directory" });
    if (selected) addRepoRoot(selected as string);
  }

  async function browseNotesVault() {
    const selected = await open({ directory: true, title: "Select Notes Vault Location" });
    if (selected) updateSetting("notesVaultRoot", selected as string);
  }
</script>

<svelte:window onkeydown={visible ? handleKey : undefined} />

{#if visible}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
    role="presentation"
    onclick={onclose}
  >
    <div
      class="relative flex max-h-[90vh] max-w-[95vw] overflow-hidden rounded-2xl border border-hairline bg-bg-deep shadow-[0_24px_64px_rgba(2,6,23,0.6),0_0_0_1px_rgba(255,255,255,0.04)]"
      style="width: {$settingsModalSize.width}px; height: {$settingsModalSize.height}px;"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      tabindex="-1"
      >
        <!-- Sidebar -->
        <aside class="flex w-[180px] shrink-0 flex-col border-r border-hairline bg-bg-surface/30 py-3">
        <div class="flex items-center gap-2 px-3 pb-2">
          <button
            aria-label="Close settings"
            class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
            onclick={onclose}
          >
            <X size={14} />
          </button>
          <div class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Settings</div>
        </div>
        <nav class="flex flex-col gap-0.5 px-2">
          {#each CATEGORIES as cat}
            {@const Icon = cat.icon}
            <button
              class="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors
                {selected === cat.id
                  ? 'bg-accent-dim text-text-primary'
                  : 'text-text-secondary hover:bg-bg-hover'}"
              onclick={() => (selected = cat.id)}
            >
              <Icon size={14} />
              <span>{cat.label}</span>
            </button>
          {/each}
        </nav>
      </aside>

      <!-- Detail pane -->
      <div class="flex min-w-0 flex-1 flex-col">
        <div class="flex h-10 shrink-0 items-center border-b border-hairline px-4">
          <h2 class="text-sm font-semibold tracking-tight">
            {CATEGORIES.find((c) => c.id === selected)?.label}
          </h2>
        </div>

        <div class="app-scrollbar flex-1 overflow-y-auto px-5 py-4">
          {#if selected === "general"}
            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-start justify-between gap-3">
                <div>
                  <div class="text-[13px]">Theme</div>
                  <div class="text-[11px] text-text-muted mt-0.5">Color preset for the app chrome. Terminal palette is configured separately under Terminal.</div>
                </div>
                <select
                  class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                  value={$settings.theme}
                  onchange={(e) => updateSetting("theme", e.currentTarget.value as typeof $settings.theme)}
                >
                  {#each THEME_DEFINITIONS as theme}
                    <option value={theme.id}>{theme.label}</option>
                  {/each}
                </select>
              </div>
              <p class="mt-2 text-[11px] text-text-muted">
                {THEME_DEFINITIONS.find((theme) => theme.id === $settings.theme)?.description}
              </p>
            </div>

            <div class="mt-4 flex items-center justify-between py-2">
              <span class="text-[13px]">Sidebar position</span>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$sidebarLayout.railSide}
                onchange={(e) => setRailSide(e.currentTarget.value as Side)}
              >
                <option value="left">Left</option>
                <option value="right">Right</option>
              </select>
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Status bar position</span>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.statusBarPosition ?? "bottom"}
                onchange={(e) => updateSetting("statusBarPosition", e.currentTarget.value as "top" | "bottom")}
              >
                <option value="top">Top</option>
                <option value="bottom">Bottom</option>
              </select>
            </div>
          {:else if selected === "sessions"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Confirm on close</div>
                <div class="text-[11px] text-text-muted mt-0.5">Prompt before closing active sessions</div>
              </div>
              <button
                aria-label="Toggle confirm on close"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.confirmOnClose ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("confirmOnClose", !$settings.confirmOnClose)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.confirmOnClose ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Restore on launch</div>
                <div class="text-[11px] text-text-muted mt-0.5">Show previous sessions on startup</div>
              </div>
              <button
                aria-label="Toggle restore sessions on launch"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.restoreSessionsOnLaunch ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("restoreSessionsOnLaunch", !$settings.restoreSessionsOnLaunch)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.restoreSessionsOnLaunch ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">On pane close</div>
                <div class="text-[11px] text-text-muted mt-0.5">Kill the terminal by default, or keep it running detached for later reconnect.</div>
              </div>
              <div class="flex overflow-hidden rounded border border-border bg-bg-deep">
                {#each PANE_CLOSE_OPTIONS as opt}
                  {@const active = ($settings.onPaneClose ?? "kill") === opt.id}
                  <button
                    class="px-2.5 py-1 text-[11px] cursor-pointer transition-colors
                      {active ? 'bg-accent-dim text-text-primary' : 'text-text-secondary hover:bg-bg-hover'}"
                    aria-pressed={active}
                    onclick={() => setPaneCloseMode(opt.id)}
                  >{opt.label}</button>
                {/each}
              </div>
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Default project path</span>
              <div class="flex gap-1">
                <input
                  class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-48 text-right focus:border-accent-dim"
                  value={$settings.defaultProjectPath ?? ""}
                  oninput={(e) => updateSetting("defaultProjectPath", e.currentTarget.value || null)}
                  placeholder="~/src"
                />
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseDefaultProject}
                >...</button>
              </div>
            </div>
            <div class="py-2">
              <div class="text-[13px]">Repository roots</div>
              <div class="text-[11px] text-text-muted mt-0.5">Quick-pick sources for New Session (keeps file picker available)</div>
              <div class="mt-2 flex gap-1">
                <input
                  class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none flex-1 focus:border-accent-dim"
                  value={repoRootDraft}
                  oninput={(e) => (repoRootDraft = e.currentTarget.value)}
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addRepoRoot(repoRootDraft);
                    }
                  }}
                  placeholder="~/src"
                />
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={() => addRepoRoot(repoRootDraft)}
                >Add</button>
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseAndAddRepoRoot}
                >...</button>
              </div>
              {#if ($settings.repoRoots ?? []).length > 0}
                <div class="mt-2 flex flex-col gap-1">
                  {#each ($settings.repoRoots ?? []) as root (root)}
                    <div class="flex items-center gap-2 rounded border border-border-subtle bg-bg-surface/35 px-2 py-1">
                      <span class="font-mono text-[11px] text-text-secondary flex-1 truncate" title={root}>{root}</span>
                      <button
                        class="text-[10px] text-text-muted hover:text-red cursor-pointer"
                        onclick={() => removeRepoRoot(root)}
                      >Remove</button>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Exclude worktrees from roots</div>
                <div class="text-[11px] text-text-muted mt-0.5">Hide linked git worktrees from root-folder quick-pick results</div>
              </div>
              <button
                aria-label="Toggle excluding worktrees from root discovery"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {($settings.excludeWorktreesFromRepoRoots ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("excludeWorktreesFromRepoRoots", !($settings.excludeWorktreesFromRepoRoots ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.excludeWorktreesFromRepoRoots ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="py-2">
              <div class="flex items-center justify-between">
                <div>
                  <div class="text-[13px]">Worktree base path</div>
                  <div class="text-[11px] text-text-muted mt-0.5">
                    Where to create new worktrees. Supports
                    <code class="font-mono">{'{project_dir}'}</code>,
                    <code class="font-mono">{'{git_root}'}</code>,
                    <code class="font-mono">{'{project_name}'}</code>,
                    <code class="font-mono">{'{home}'}</code>.
                  </div>
                </div>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.worktreeBasePath ?? ""}
                    oninput={(e) => updateSetting("worktreeBasePath", e.currentTarget.value || null)}
                    placeholder="{'{project_dir}'}/.worktrees"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseWorktreeBase}
                  >...</button>
                </div>
              </div>
              {#if previewText}
                <div class="mt-1.5 text-[11px] text-text-muted font-mono truncate" title={previewText}>
                  → {previewText}
                </div>
              {/if}
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">On session close</div>
                <div class="text-[11px] text-text-muted mt-0.5">What to do with the session's worktree</div>
              </div>
              <div class="flex rounded border border-border bg-bg-deep overflow-hidden">
                {#each [
                  { id: "never", label: "Keep" },
                  { id: "prompt", label: "Ask" },
                  { id: "always", label: "Remove" },
                ] as const as opt}
                  {@const active = ($settings.worktreeCleanupOnClose ?? "prompt") === opt.id}
                  <button
                    class="px-2.5 py-1 text-[11px] cursor-pointer transition-colors
                      {active ? 'bg-accent-dim text-text-primary' : 'text-text-secondary hover:bg-bg-hover'}"
                    onclick={() => setCleanupMode(opt.id)}
                  >{opt.label}</button>
                {/each}
              </div>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">New Worktree default</div>
                <div class="text-[11px] text-text-muted mt-0.5">Default starting point for new worktree branches — applies to the New Session dialog and the "New Worktree" context-menu click. Hover / command palette always expose all three.</div>
              </div>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.worktreeDefaultBase ?? "currentBranch"}
                onchange={(e) => setDefaultBase(e.currentTarget.value as WorktreeDefaultBase)}
              >
                <option value="currentBranch">Current branch</option>
                <option value="main">main</option>
                <option value="originMain">origin/main</option>
              </select>
            </div>
          {:else if selected === "kanban"}
            {@const kanban = kanbanSettings()}
            <div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-start justify-between gap-3">
                <div>
                  <div class="text-[13px]">Default agent</div>
                  <div class="mt-0.5 text-[11px] text-text-muted">Used when a card has no agent profile. Card settings and command arguments still win.</div>
                </div>
                <select
                  class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6 max-w-[14rem]"
                  value={kanban.defaultAgentProfile}
                  onchange={(e) => updateKanban("defaultAgentProfile", e.currentTarget.value)}
                >
                  {#if autonomousProfiles.length === 0}
                    <option value="claude">Claude</option>
                  {:else}
                    {#each autonomousProfiles as profile (profile.id)}
                      <option value={profile.id}>{profile.name}</option>
                    {/each}
                  {/if}
                </select>
              </div>
            </div>

            <div class="mt-4 flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Open on launch</div>
                <div class="mt-0.5 text-[11px] text-text-muted">Choose which sidebar Roux shows after settings load.</div>
              </div>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={kanban.startupSidebar}
                onchange={(e) => updateKanban("startupSidebar", e.currentTarget.value as KanbanStartupSidebar)}
              >
                <option value="restore">Restore previous</option>
                <option value="sessions">Sessions</option>
                <option value="kanban">Kanban</option>
                <option value="none">None</option>
              </select>
            </div>

            <div class="py-2">
              <div class="text-[13px]">Planning instructions</div>
              <div class="mt-0.5 text-[11px] text-text-muted">Appended after Roux's required planning prompt.</div>
              <textarea
                class="mt-2 min-h-24 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
                value={kanban.planningPromptAppend}
                oninput={(e) => updateKanban("planningPromptAppend", e.currentTarget.value)}
              ></textarea>
            </div>

            <div class="py-2">
              <div class="text-[13px]">Implementation instructions</div>
              <div class="mt-0.5 text-[11px] text-text-muted">Appended after Roux's required Start prompt.</div>
              <textarea
                class="mt-2 min-h-24 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
                value={kanban.implementationPromptAppend}
                oninput={(e) => updateKanban("implementationPromptAppend", e.currentTarget.value)}
              ></textarea>
            </div>

            <div class="py-2">
              <div class="text-[13px]">Review handoff instructions</div>
              <div class="mt-0.5 text-[11px] text-text-muted">Included in the implementation prompt until automated review runs exist.</div>
              <textarea
                class="mt-2 min-h-24 w-full resize-y rounded border border-border bg-bg-deep px-2 py-1.5 text-xs text-text-primary outline-none focus:border-accent-dim"
                value={kanban.reviewPromptAppend}
                oninput={(e) => updateKanban("reviewPromptAppend", e.currentTarget.value)}
              ></textarea>
            </div>
          {:else if selected === "terminal"}
            {@const allTerminalThemes = getAllTerminalThemeDefinitions($userTerminalThemes)}
            {@const currentTerminalThemeId = $settings.terminalTheme ?? "match-gui"}
            {@const currentDef = allTerminalThemes.find((t) => t.id === currentTerminalThemeId)}
            {@const isMissingUserTheme = !currentDef && currentTerminalThemeId.startsWith("user:")}
            <div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3 mb-3">
              <div class="flex items-start justify-between gap-3">
                <div>
                  <div class="text-[13px]">Terminal theme</div>
                  <div class="text-[11px] text-text-muted mt-0.5">Color palette for the xterm pane. Independent of the GUI theme. Save iTerm2 <code>.itermcolors</code> files into <code>~/.config/roux/themes/</code> to add your own.</div>
                </div>
                <div class="flex items-center gap-1">
                  <select
                    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6 max-w-[14rem]"
                    value={currentTerminalThemeId}
                    onchange={(e) => updateSetting("terminalTheme", e.currentTarget.value)}
                  >
                    <optgroup label="Auto">
                      {#each allTerminalThemes.filter((t) => t.category === "auto") as t}
                        <option value={t.id}>{t.label}</option>
                      {/each}
                    </optgroup>
                    <optgroup label="App theme palettes">
                      {#each allTerminalThemes.filter((t) => t.category === "matching") as t}
                        <option value={t.id}>{t.label}</option>
                      {/each}
                    </optgroup>
                    <optgroup label="Editor themes">
                      {#each allTerminalThemes.filter((t) => t.category === "editor") as t}
                        <option value={t.id}>{t.label}</option>
                      {/each}
                    </optgroup>
                    {#if $userTerminalThemes.length > 0}
                      <optgroup label="User">
                        {#each allTerminalThemes.filter((t) => t.category === "user") as t}
                          <option value={t.id}>{t.label}</option>
                        {/each}
                      </optgroup>
                    {/if}
                    {#if isMissingUserTheme}
                      <!-- Persisted theme references a user file that's not
                           present right now (deleted, renamed, or themes
                           folder hasn't loaded yet). Surface it as a
                           disabled option so the dropdown reflects the
                           setting; selecting any other entry overwrites it. -->
                      <option value={currentTerminalThemeId} disabled>
                        Missing: {currentTerminalThemeId.slice("user:".length)}
                      </option>
                    {/if}
                  </select>
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    title="Open ~/.config/roux/themes/ in the file manager"
                    onclick={async () => {
                      try {
                        const dir = await commands.userThemesDir();
                        await revealItemInDir(dir);
                      } catch (e) {
                        console.error("reveal user themes dir failed", e);
                      }
                    }}
                  >Reveal</button>
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    title="Re-scan ~/.config/roux/themes/"
                    onclick={() => void loadUserTerminalThemes()}
                  >Reload</button>
                </div>
              </div>
              {#if isMissingUserTheme}
                <p class="mt-2 text-[11px] text-amber-500/90">
                  This theme file isn't currently loaded. The setting is preserved — drop the file back into <code>~/.config/roux/themes/</code> and hit Reload, or pick a different theme.
                </p>
              {:else if currentDef?.description}
                <p class="mt-2 text-[11px] text-text-muted">{currentDef.description}</p>
              {/if}
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Font size</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-20 text-right focus:border-accent-dim"
                type="number"
                value={$settings.fontSize}
                oninput={(e) => updateSetting("fontSize", parseInt(e.currentTarget.value) || 14)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Terminal font</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
                value={$settings.fontFamily}
                oninput={(e) => updateSetting("fontFamily", e.currentTarget.value)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">UI font</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
                value={$settings.uiFontFamily}
                oninput={(e) => updateSetting("uiFontFamily", e.currentTarget.value)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Scrollback lines</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none w-24 text-right focus:border-accent-dim"
                type="number"
                value={$settings.scrollback}
                oninput={(e) => updateSetting("scrollback", parseInt(e.currentTarget.value) || 5000)}
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">GPU acceleration</div>
                <div class="text-[11px] text-text-muted mt-0.5">Applies to terminals opened after this change.</div>
              </div>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.gpuAcceleration ?? "auto"}
                onchange={(e) => updateSetting("gpuAcceleration", e.currentTarget.value as GpuAcceleration)}
              >
                <option value="auto">Auto</option>
                <option value="on">On (WebGL)</option>
                <option value="off">Off (DOM)</option>
              </select>
            </div>
          {:else if selected === "claude"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Binary path</div>
                <div class="text-[11px] text-text-muted mt-0.5">Leave blank to auto-detect from PATH</div>
              </div>
              <div class="flex gap-1">
                <input
                  class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-48 text-right focus:border-accent-dim"
                  value={$settings.claudeBinaryPath ?? ""}
                  oninput={(e) => updateSetting("claudeBinaryPath", e.currentTarget.value || null)}
                  placeholder="/usr/local/bin/claude"
                />
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseClaudeBinary}
                >...</button>
              </div>
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Default model</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-40 text-right focus:border-accent-dim"
                value={$settings.defaultModel ?? ""}
                oninput={(e) => updateSetting("defaultModel", e.currentTarget.value || null)}
                placeholder="opus"
              />
            </div>
            <div class="flex items-center justify-between py-2">
              <span class="text-[13px]">Additional flags</span>
              <input
                class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-56 text-right focus:border-accent-dim"
                value={$settings.additionalFlags.join(" ")}
                oninput={(e) => updateSetting("additionalFlags", e.currentTarget.value.split(" ").filter(Boolean))}
                placeholder="--verbose"
              />
            </div>
          {:else if selected === "notes"}
            <div class="py-2">
              <div class="flex items-center justify-between">
                <div>
                  <div class="text-[13px]">Vault location</div>
                  <div class="text-[11px] text-text-muted mt-0.5">Where Roux stores notes. Works as a standalone folder or a subdirectory inside an Obsidian vault.</div>
                </div>
              </div>
              <div class="mt-2 flex gap-1">
                <input
                  class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none flex-1 focus:border-accent-dim"
                  value={$settings.notesVaultRoot ?? ""}
                  oninput={(e) => updateSetting("notesVaultRoot", e.currentTarget.value || null)}
                  placeholder="~/Documents/Roux"
                />
                <button
                  class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                  onclick={browseNotesVault}
                >...</button>
              </div>
              <div class="mt-1.5 text-[11px] text-text-muted">
                Leave blank to use the default location. Changing this does not move existing notes.
              </div>
            </div>
            <div class="flex items-center justify-between py-2 mt-2">
              <div>
                <div class="text-[13px]">Include web anchors</div>
                <div class="text-[11px] text-text-muted mt-0.5">Add HTML anchor tags for compatibility with static site generators. Disable for cleaner markdown in Obsidian.</div>
              </div>
              <button
                aria-label="Toggle web anchors in notes"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {($settings.notesIncludeWebAnchors ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("notesIncludeWebAnchors", !($settings.notesIncludeWebAnchors ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.notesIncludeWebAnchors ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
          {:else if selected === "integrations"}
            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between">
                <div class="text-[13px] font-semibold">Roux MCP</div>
                {#if mcpStatus?.enabled}
                  <span class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green">enabled</span>
                {:else}
                  <span class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted">off</span>
                {/if}
              </div>
              <div class="mt-0.5 text-[11px] text-text-muted">
                Lets MCP clients launch <code class="font-mono">roux mcp</code> and use Roux sessions, panes, and notes through the running app.
              </div>
              <div class="mt-3 flex items-center justify-between py-1">
                <div>
                  <div class="text-[13px]">Enable Roux MCP</div>
                  <div class="mt-0.5 text-[11px] text-text-muted">Safe action tools are available by default; destructive tools are not exposed.</div>
                </div>
                <button
                  aria-label="Toggle Roux MCP"
                  class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                    {($settings.mcpEnabled ?? false) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                  onclick={() => updateSetting("mcpEnabled", !($settings.mcpEnabled ?? false))}
                >
                  <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                    {($settings.mcpEnabled ?? false) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
                </button>
              </div>
              {#if mcpStatus}
                <div class="mt-2 grid grid-cols-2 gap-2 text-[11px]">
                  <div class="rounded border border-border-subtle bg-bg-deep/70 px-2 py-1">
                    <div class="text-text-muted">CLI</div>
                    <div class={mcpStatus.cliInstalled && mcpStatus.cliCurrent ? "text-green" : "text-red"}>
                      {mcpStatus.cliInstalled ? (mcpStatus.cliCurrent ? "current" : "stale") : "missing"}
                    </div>
                  </div>
                  <div class="rounded border border-border-subtle bg-bg-deep/70 px-2 py-1">
                    <div class="text-text-muted">Server path</div>
                    <div class="truncate font-mono text-text-secondary" title={mcpStatus.cliPath}>{mcpStatus.cliPath}</div>
                  </div>
                  <div class="col-span-2 rounded border border-border-subtle bg-bg-deep/70 px-2 py-1">
                    <div class="text-text-muted">Last config update</div>
                    <div class="text-text-secondary">
                      {mcpStatus.lastConfiguredAtMs ? formatMcpConfiguredAt(mcpStatus.lastConfiguredAtMs) : "Never"}
                      {mcpStatus.lastConfiguredHost ? ` · ${mcpStatus.lastConfiguredHost}` : ""}
                    </div>
                  </div>
                </div>
              {/if}
              {#if mcpStatus}
                {#each mcpStatus.hosts as host (host.id)}
                  {@const hostId = host.id as McpHostIdT}
                  {@const busy = mcpBusyByHost[hostId] ?? null}
                  {@const message = mcpMessageByHost[hostId] ?? null}
                  {@const error = mcpErrorByHost[hostId] ?? null}
                  {@const preview = mcpPreviewByHost[hostId] ?? null}
                  <div class="mt-3 rounded border border-border-subtle bg-bg-deep/60 p-2">
                    <div class="flex items-center justify-between gap-2">
                      <div>
                        <div class="text-[12px] font-medium">{host.label}</div>
                        <div class="mt-0.5 max-w-[22rem] truncate font-mono text-[10px] text-text-muted" title={host.configPath ?? ""}>
                          {host.configPath ?? "Config path unavailable"}
                        </div>
                      </div>
                      {#if host.configured}
                        <span class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green">configured</span>
                      {:else if host.error}
                        <span class="rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red">needs attention</span>
                      {:else}
                        <span class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted">not configured</span>
                      {/if}
                    </div>
                    <div class="mt-2 flex gap-1">
                      <button
                        class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
                        disabled={!($settings.mcpEnabled ?? false) || busy !== null}
                        onclick={() => previewMcpHostConfig(hostId, host.label)}
                      >{busy === "preview" ? "Previewing" : "Preview"}</button>
                      <button
                        class="rounded border border-accent-dim bg-accent/15 px-2 py-1 text-[11px] text-text-primary hover:bg-accent/25 disabled:cursor-not-allowed disabled:opacity-40"
                        disabled={!($settings.mcpEnabled ?? false) || busy !== null}
                        onclick={() => configureMcpHost(hostId, host.label)}
                      >{busy === "configure" ? "Adding…" : `Add to ${host.label}`}</button>
                    </div>
                    {#if message}
                      <div class="mt-2 text-[11px] text-green">{message}</div>
                    {/if}
                    {#if error || host.error}
                      <div class="mt-2 text-[11px] text-red">{error ?? host.error}</div>
                    {/if}
                    {#if preview}
                      <div class="mt-2 rounded border border-border-subtle bg-bg-deep/70 p-2">
                        <div class="mb-1 text-[10px] uppercase tracking-wider text-text-muted">Roux entry</div>
                        <pre class="app-scrollbar max-h-28 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] text-text-secondary">{preview.nextEntryJson}</pre>
                      </div>
                    {/if}
                  </div>
                {/each}
              {/if}
            </div>

            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between gap-2">
                <div>
                  <div class="text-[13px] font-semibold">External Tools</div>
                  <div class="mt-0.5 text-[11px] text-text-muted">
                    Launch terminal and local web tools into the main view.
                  </div>
                </div>
                <div class="flex gap-1">
                  <button
                    type="button"
                    class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
                    onclick={() => addExternalTool("terminal")}
                  >Add Terminal</button>
                  <button
                    type="button"
                    class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
                    onclick={() => addExternalTool("web")}
                  >Add Web</button>
                </div>
              </div>

              <div class="mt-3 flex flex-col gap-2">
                {#each externalTools() as tool (externalToolRowKey(tool.id))}
                  {@const expanded = expandedExternalToolId === tool.id}
                  {@const preview = externalToolPreviewById[tool.id]}
                  <div class="rounded border border-border-subtle bg-bg-deep/60 p-2">
                    <div class="flex items-center gap-2">
                      <button
                        type="button"
                        class="min-w-0 flex-1 truncate text-left text-[12px] font-medium text-text-primary"
                        onclick={() => (expandedExternalToolId = expanded ? null : tool.id)}
                      >
                        {tool.name}
                      </button>
                      <span class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-text-muted">
                        {tool.surface ?? "terminal"}
                      </span>
                      <label class="flex items-center gap-1 text-[11px] text-text-muted">
                        <input
                          type="checkbox"
                          class="h-3 w-3 accent-accent"
                          checked={tool.enabled !== false}
                          onchange={(e) => updateExternalTool(tool.id, { enabled: e.currentTarget.checked })}
                        />
                        Enabled
                      </label>
                      <button
                        type="button"
                        class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-red"
                        onclick={() => removeExternalTool(tool.id)}
                      >Remove</button>
                    </div>

                    {#if expanded}
                      <div class="mt-3 grid gap-2">
                        <label class="grid gap-1 text-[11px] text-text-muted">
                          <span>Name</span>
                          <input
                            class="rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-primary outline-none focus:border-accent-dim"
                            value={tool.name}
                            oninput={(e) => updateExternalTool(tool.id, { name: e.currentTarget.value })}
                          />
                        </label>
                        <div class="grid gap-2 md:grid-cols-[1fr_1fr]">
                          <label class="grid gap-1 text-[11px] text-text-muted">
                            <span>ID</span>
                            <input
                              class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                              value={tool.id}
                              oninput={(e) => updateExternalTool(tool.id, { id: e.currentTarget.value })}
                            />
                          </label>
                          <label class="grid gap-1 text-[11px] text-text-muted">
                            <span>Surface</span>
                            <input
                              class="rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-secondary"
                              value={tool.surface ?? "terminal"}
                              readonly
                            />
                          </label>
                        </div>
                        <label class="grid gap-1 text-[11px] text-text-muted">
                          <span>Command template{(tool.surface ?? "terminal") === "web" ? " (optional)" : ""}</span>
                          <textarea
                            class="min-h-16 rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                            value={tool.commandTemplate}
                            oninput={(e) => updateExternalTool(tool.id, { commandTemplate: e.currentTarget.value })}
                          ></textarea>
                        </label>
                        <label class="grid gap-1 text-[11px] text-text-muted">
                          <span>CWD template</span>
                          <input
                            class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                            value={tool.cwdTemplate ?? ""}
                            oninput={(e) => updateExternalTool(tool.id, { cwdTemplate: e.currentTarget.value })}
                          />
                        </label>
                        {#if (tool.surface ?? "terminal") === "web"}
                          <div class="grid gap-1 text-[11px] text-text-muted">
                            <span>Embedder</span>
                            <div class="inline-flex w-fit overflow-hidden rounded border border-border bg-bg-deep">
                              {#each [{ value: "iframe", label: "Iframe" }, { value: "webview", label: "Webview" }] as option}
                                <button
                                  type="button"
                                  class="px-2 py-1 text-[11px] transition-colors {tool.webEmbedder === option.value ? 'bg-bg-active text-text-primary' : 'text-text-muted hover:bg-bg-hover hover:text-text-secondary'}"
                                  onclick={() => updateExternalTool(tool.id, { webEmbedder: option.value as ExternalToolWebEmbedder })}
                                >
                                  {option.label}
                                </button>
                              {/each}
                            </div>
                          </div>
                          <div class="grid gap-2 md:grid-cols-[1fr_120px]">
                            <label class="grid gap-1 text-[11px] text-text-muted">
                              <span>URL template</span>
                              <input
                                class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                                value={tool.urlTemplate ?? ""}
                                oninput={(e) => updateExternalTool(tool.id, { urlTemplate: e.currentTarget.value || null })}
                              />
                            </label>
                            <label class="grid gap-1 text-[11px] text-text-muted">
                              <span>Preferred port</span>
                              <input
                                type="number"
                                min="1"
                                max="65535"
                                class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                                value={tool.preferredPort ?? ""}
                                oninput={(e) => updateExternalTool(tool.id, { preferredPort: preferredPortFromInput(e.currentTarget.value) })}
                              />
                            </label>
                          </div>
                        {/if}
                        <label class="flex items-center gap-2 text-[11px] text-text-secondary">
                          <input
                            type="checkbox"
                            class="h-3 w-3 accent-accent"
                            checked={tool.requiresSession ?? false}
                            onchange={(e) => updateExternalTool(tool.id, { requiresSession: e.currentTarget.checked })}
                          />
                          Requires active session
                        </label>
                        <div class="flex items-center gap-2">
                          <button
                            type="button"
                            class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:opacity-40"
                            disabled={preview?.loading}
                            onclick={() => void previewTool(tool)}
                          >{preview?.loading ? "Previewing" : "Preview Render"}</button>
                          {#if tool.requiresSession && !$activeSession}
                            <span class="text-[11px] text-amber">Preview needs an active session.</span>
                          {/if}
                        </div>
                        {#if preview?.error}
                          <div class="rounded border border-red/25 bg-red/10 p-2 text-[11px] text-red">
                            {preview.error}
                          </div>
                        {:else if preview?.rendered}
                          <div class="grid gap-1 rounded border border-border-subtle bg-bg-deep/70 p-2 font-mono text-[10px] text-text-secondary">
                            <div><span class="text-text-muted">cmd</span> {preview.rendered.command}</div>
                            <div><span class="text-text-muted">cwd</span> {preview.rendered.cwd}</div>
                            {#if preview.rendered.url}
                              <div><span class="text-text-muted">url</span> {preview.rendered.url}</div>
                            {/if}
                          </div>
                        {/if}
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            </div>

            <div class="rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between">
                <div class="text-[13px] font-semibold">Shell</div>
              </div>
              <div class="mt-0.5 text-[11px] text-text-muted">
                Shell used for terminal panes and login-shell PATH discovery
                (for finding <code class="font-mono">gh</code>, <code class="font-mono">git</code>,
                <code class="font-mono">wt</code>, etc. via Homebrew). Defaults to your OS login shell,
                then <code class="font-mono">$SHELL</code>. Set this only if auto-detection chooses the
                wrong shell. New terminal panes use the updated shell right away; restart Roux if
                integration PATH discovery needs to be refreshed.
              </div>
              <div class="mt-3 flex items-center justify-between gap-2">
                <span class="text-[13px]">Binary path</span>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.shellBinaryPath ?? ""}
                    oninput={(e) => updateSetting("shellBinaryPath", e.currentTarget.value || null)}
                    placeholder="/opt/homebrew/bin/fish"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseShellBinary}
                  >...</button>
                </div>
              </div>
            </div>

            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between">
                <div class="text-[13px] font-semibold">GitHub CLI</div>
                {#if ghDetection?.binaryPath}
                  <span
                    class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
                    >detected{ghDetection.version ? ` ${ghDetection.version}` : ""}</span
                  >
                {:else if ghDetection !== null}
                  <span
                    class="rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red"
                    >missing</span
                  >
                {/if}
              </div>
              <div class="mt-0.5 text-[11px] text-text-muted">
                Used for "Session from PR" and PR watches. Roux auto-detects
                <code class="font-mono">gh</code> via your login shell's PATH (including fish). Set this only if
                auto-detection misses your install — paste the output of <code class="font-mono">which gh</code>.
              </div>
              {#if ghDetection?.binaryPath}
                <div class="mt-2 font-mono text-[10px] text-text-muted">
                  {ghDetection.binaryPath}
                </div>
              {/if}
              <div class="mt-3 flex items-center justify-between gap-2">
                <span class="text-[13px]">Binary path</span>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.ghBinaryPath ?? ""}
                    oninput={(e) => updateSetting("ghBinaryPath", e.currentTarget.value || null)}
                    placeholder="/opt/homebrew/bin/gh"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseGhBinary}
                  >...</button>
                </div>
              </div>
              <div class="mt-3 flex items-center justify-between py-1">
                <div>
                  <div class="text-[13px]">Look up PR for session branch</div>
                  <div class="text-[11px] text-text-muted mt-0.5">
                    Run <code class="font-mono">gh pr list --head &lt;branch&gt;</code> on session activation
                    so the status bar can show a clickable PR link.
                  </div>
                </div>
                <button
                  aria-label="Toggle session PR lookup"
                  class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                    {$settings.autoLookupSessionPr !== false ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                  onclick={() => updateSetting("autoLookupSessionPr", $settings.autoLookupSessionPr === false)}
                >
                  <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                    {$settings.autoLookupSessionPr !== false ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
                </button>
              </div>
              <div class="flex items-center justify-between py-1">
                <div>
                  <div class="text-[13px]">Auto-create PR watch for sessions</div>
                  <div class="text-[11px] text-text-muted mt-0.5">
                    When the lookup finds a PR, create a session-scoped GitHub PR watch
                    automatically. Requires the lookup above.
                  </div>
                </div>
                <button
                  aria-label="Toggle auto PR watch"
                  disabled={$settings.autoLookupSessionPr === false}
                  class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                    {$settings.autoWatchSessionPr ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}
                    {$settings.autoLookupSessionPr === false ? 'opacity-40 cursor-not-allowed' : ''}"
                  onclick={() => updateSetting("autoWatchSessionPr", !$settings.autoWatchSessionPr)}
                >
                  <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                    {$settings.autoWatchSessionPr ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
                </button>
              </div>
            </div>

            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between">
                <div class="text-[13px] font-semibold">Git</div>
                {#if gitDetection?.binaryPath}
                  <span
                    class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
                    >detected{gitDetection.version ? ` ${gitDetection.version}` : ""}</span
                  >
                {:else if gitDetection !== null}
                  <span
                    class="rounded bg-red/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-red"
                    >missing</span
                  >
                {/if}
              </div>
              <div class="mt-0.5 text-[11px] text-text-muted">
                Used for git-backed Library sources. Roux checks this override,
                then <code class="font-mono">ROUX_GIT</code>, then your login shell's PATH, then the app PATH.
                Set this only if auto-detection misses your install.
              </div>
              {#if gitDetection?.binaryPath}
                <div class="mt-2 font-mono text-[10px] text-text-muted">
                  {gitDetection.binaryPath}
                </div>
              {/if}
              <div class="mt-3 flex items-center justify-between gap-2">
                <span class="text-[13px]">Binary path</span>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.gitBinaryPath ?? ""}
                    oninput={(e) => updateSetting("gitBinaryPath", e.currentTarget.value || null)}
                    placeholder="/opt/homebrew/bin/git"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseGitBinary}
                  >...</button>
                </div>
              </div>
            </div>

            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between">
                <div class="text-[13px] font-semibold">Worktrunk (wt)</div>
                {#if worktrunkDetection?.binaryPath}
                  <span
                    data-testid="worktrunk-detected-badge"
                    class="rounded bg-green/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-green"
                    >detected{worktrunkDetection.version
                      ? ` ${worktrunkDetection.version}`
                      : ""}</span
                  >
                {:else}
                  <span
                    data-testid="worktrunk-not-detected-badge"
                    class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted"
                    >not detected</span
                  >
                {/if}
              </div>
              <div class="mt-0.5 text-[11px] text-text-muted">
                When available, Roux enriches the New Session worktree picker
                with <code class="font-mono">wt</code>'s richer metadata (dirty
                state, ahead/behind, locked/prunable, current/previous). Opt-in
                — no regression for users without <code class="font-mono">wt</code>.
                Set the path only if auto-detection misses your install.
              </div>
              {#if worktrunkDetection?.binaryPath}
                <div class="mt-2 font-mono text-[10px] text-text-muted">
                  {worktrunkDetection.binaryPath}
                </div>
              {/if}
              <div class="mt-3 flex items-center justify-between gap-2">
                <span class="text-[13px]">Binary path</span>
                <div class="flex gap-1">
                  <input
                    class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-64 text-right focus:border-accent-dim"
                    value={$settings.worktrunkBinaryPath ?? ""}
                    oninput={(e) =>
                      updateSetting(
                        "worktrunkBinaryPath",
                        e.currentTarget.value || null
                      )}
                    placeholder="/opt/homebrew/bin/wt"
                  />
                  <button
                    class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
                    onclick={browseWorktrunkBinary}>...</button
                  >
                </div>
              </div>
              <div class="mt-3 flex items-center justify-between gap-2">
                <div>
                  <div class="text-[13px]">Worktree provider</div>
                  <div class="mt-0.5 text-[11px] text-text-muted">
                    How Roux creates new worktrees. Auto uses <code
                      class="font-mono">wt</code
                    > when detected and falls back to git otherwise.
                  </div>
                </div>
                <div
                  data-testid="worktrunk-provider-selector"
                  class="flex overflow-hidden rounded border border-border bg-bg-deep"
                >
                  {#each [
                    { id: "auto", label: "Auto" },
                    { id: "git", label: "Git" },
                    { id: "worktrunk", label: "wt" },
                  ] as const as opt}
                    {@const active =
                      ($settings.worktreeProvider ?? "auto") === opt.id}
                    <button
                      data-testid={`worktrunk-provider-${opt.id}`}
                      class="cursor-pointer px-2.5 py-1 text-[11px] transition-colors
                        {active
                        ? 'bg-accent-dim text-text-primary'
                        : 'text-text-secondary hover:bg-bg-hover'}"
                      onclick={() =>
                        updateSetting(
                          "worktreeProvider",
                          opt.id as WorktreeProvider
                        )}>{opt.label}</button
                    >
                  {/each}
                </div>
              </div>
            </div>
          {:else if selected === "notifications"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Enable OS notifications</div>
                <div class="text-[11px] text-text-muted mt-0.5">Master switch for macOS notification fan-out. The in-app pane always works.</div>
              </div>
              <button
                aria-label="Toggle OS notifications"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.notificationsEnabled ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("notificationsEnabled", !$settings.notificationsEnabled)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.notificationsEnabled ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>

            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Agent completion notifications</div>
                <div class="text-[11px] text-text-muted mt-0.5">Notify when an agent finishes in a pane other than the one you're focused on. Errors notify regardless.</div>
              </div>
              <button
                aria-label="Toggle agent completion notifications"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {($settings.agentCompletionNotificationsEnabled ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("agentCompletionNotificationsEnabled", !($settings.agentCompletionNotificationsEnabled ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.agentCompletionNotificationsEnabled ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>

            <div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
              <div class="flex items-center justify-between gap-2">
                <div>
                  <div class="text-[13px] font-semibold">Agent notifications</div>
                  <div class="mt-0.5 text-[11px] text-text-muted">
                    Configure Claude Code hooks and Codex TUI settings so agent events reach Roux.
                  </div>
                </div>
                <button
                  class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
                  disabled={agentNotificationBusy !== null}
                  onclick={() => {
                    agentNotificationBusy = "refresh";
                    agentNotificationMessage = null;
                    const run = ++agentNotificationStatusRun;
                    void refreshAgentNotificationStatus(run).finally(() => {
                      if (agentNotificationBusy === "refresh") agentNotificationBusy = null;
                    });
                  }}
                >{agentNotificationBusy === "refresh" ? "Refreshing" : "Refresh"}</button>
              </div>

              <div class="mt-3 flex flex-col gap-2">
                <div class="rounded border border-border-subtle bg-bg-deep/60 p-2">
                  <div class="flex items-start justify-between gap-2">
                    <div class="min-w-0">
                      <div class="text-[12px] font-medium">Claude Code</div>
                      <div class="mt-0.5 text-[11px] text-text-muted">
                        {claudeNotificationProvider?.detail ?? "Checking Claude Code hook setup."}
                      </div>
                    </div>
                    <span class={notificationProviderClass(claudeNotificationProvider)}>
                      {notificationProviderLabel(claudeNotificationProvider)}
                    </span>
                  </div>
                  <div class="mt-2 flex gap-1">
                    <button
                      class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
                      disabled={agentNotificationBusy !== null || !claudeNotificationProvider || claudeNotificationProvider.installable === false}
                      onclick={configureClaudeNotifications}
                    >{agentNotificationBusy === "claude" ? "Configuring" : (claudeNotificationProvider?.status === "installed" ? "Reinstall" : "Configure")}</button>
                  </div>
                </div>

                <div class="rounded border border-border-subtle bg-bg-deep/60 p-2">
                  <div class="flex items-start justify-between gap-2">
                    <div class="min-w-0">
                      <div class="text-[12px] font-medium">Codex</div>
                      <div class="mt-0.5 text-[11px] text-text-muted">
                        {codexNotificationProvider?.detail ?? "Checking Codex notification configuration."}
                      </div>
                      {#if codexNotificationProvider?.configPath}
                        <div class="mt-1 max-w-[25rem] truncate font-mono text-[10px] text-text-muted" title={codexNotificationProvider.configPath}>
                          {codexNotificationProvider.configPath}
                        </div>
                      {/if}
                    </div>
                    <span class={notificationProviderClass(codexNotificationProvider)}>
                      {notificationProviderLabel(codexNotificationProvider)}
                    </span>
                  </div>
                  <div class="mt-2 flex gap-1">
                    <button
                      class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
                      disabled={agentNotificationBusy !== null || !codexNotificationProvider || codexNotificationProvider.installable === false}
                      onclick={previewCodexNotifications}
                    >{agentNotificationBusy === "codex-preview" ? "Previewing" : "Preview"}</button>
                    <button
                      class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-40"
                      disabled={agentNotificationBusy !== null || !codexNotificationProvider || codexNotificationProvider.installable === false}
                      onclick={configureCodexNotifications}
                    >{agentNotificationBusy === "codex-configure" ? "Configuring" : "Configure"}</button>
                  </div>

                  {#if codexNotificationPreview}
                    <div class="mt-2 rounded border border-border-subtle bg-bg-deep/70 p-2">
                      <div class="mb-1 flex items-center justify-between gap-2 text-[10px] uppercase tracking-wider text-text-muted">
                        <span>Codex config preview</span>
                        <span class="truncate font-mono normal-case tracking-normal" title={codexNotificationPreview.configPath}>
                          {codexNotificationPreview.configPath}
                        </span>
                      </div>
                      <pre class="app-scrollbar max-h-32 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] text-text-secondary">{codexNotificationPreview.nextContent}</pre>
                    </div>
                  {/if}
                </div>
              </div>

              {#if agentNotificationMessage}
                <div class="mt-2 text-[11px] text-green">{agentNotificationMessage}</div>
              {/if}
              {#if agentNotificationError}
                <div class="mt-2 text-[11px] text-red">{agentNotificationError}</div>
              {/if}
            </div>

            <div class="mt-3 rounded-lg border border-amber/20 bg-amber/5 p-3 text-[11px] text-text-secondary">
              <div class="mb-1 font-semibold text-amber">macOS quirk</div>
              <div class="leading-relaxed">
                In dev mode Roux borrows <span class="font-mono text-[10px]">com.apple.Terminal</span>'s notification identity (unsigned binaries can't own a bundle id). If you don't see the test notification below, open <span class="text-text-primary">System Settings → Notifications → Terminal</span> and make sure "Allow Notifications" is on. Bundled release builds use <span class="font-mono text-[10px]">com.phin-tech.roux</span>.
              </div>
            </div>

            <div class="mt-3 flex items-center justify-between gap-3">
              <div>
                <div class="text-[13px]">Test notification</div>
                <div class="text-[11px] text-text-muted mt-0.5">
                  {#if notifTestStatus === "sent"}
                    Test fired — check macOS notification center. If nothing shows, fix permissions above.
                  {:else if notifTestStatus === "error"}
                    <span class="text-red">Failed: {notifTestError}</span>
                  {:else}
                    Pushes an Attention-level notification through the service
                  {/if}
                </div>
              </div>
              <button
                class="shrink-0 cursor-pointer rounded-lg border border-border-subtle bg-bg-deep px-3 py-1.5 text-[12px] font-semibold text-text-primary hover:border-accent hover:bg-bg-hover"
                onclick={sendTestNotification}
              >
                Send test
              </button>
            </div>
          {:else if selected === "keyboard"}
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Show pane hint overlay when holding Option</div>
                <div class="text-[11px] text-text-muted mt-0.5">Reveals pane numbers while ⌥ is held. Option+digit shortcuts still work either way.</div>
              </div>
              <button
                aria-label="Toggle pane hint overlay on Option"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.showPaneHintsOnOption ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("showPaneHintsOnOption", !$settings.showPaneHintsOnOption)}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.showPaneHintsOnOption ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Show session hint overlay when holding Command</div>
                <div class="text-[11px] text-text-muted mt-0.5">Reveals session shortcuts while ⌘ is held. Command chord shortcuts still work either way.</div>
              </div>
              <button
                aria-label="Toggle session hint overlay on Command"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.showSessionHintsOnCommand !== false ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting("showSessionHintsOnCommand", !($settings.showSessionHintsOnCommand !== false))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.showSessionHintsOnCommand !== false ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
          {:else if selected === "experiments"}
            <p class="text-[11px] text-text-muted mb-3">
              Toggle in-progress features. Experiments default to off and may change behavior, persistence, or performance. Disable if you hit issues.
            </p>
            {#each EXPERIMENTS as exp (exp.id)}
              <div class="flex items-start justify-between gap-3 py-2">
                <div>
                  <div class="text-[13px]">{exp.label}</div>
                  <div class="text-[11px] text-text-muted mt-0.5">{exp.description}</div>
                </div>
                {#if exp.kind === "boolean"}
                  {@const current = currentExperimentValue($settings.experiments, exp.id) as boolean}
                  <button
                    aria-label="Toggle {exp.label}"
                    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border shrink-0
                      {current ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                    onclick={() =>
                      updateSetting(
                        "experiments",
                        withExperimentValue($settings.experiments, exp.id, !current),
                      )}
                  >
                    <div
                      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                        {current ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"
                    ></div>
                  </button>
                {:else}
                  {@const current = currentExperimentValue($settings.experiments, exp.id) as string}
                  <select
                    aria-label="Select {exp.label}"
                    class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6 shrink-0"
                    value={current}
                    onchange={(e) =>
                      updateSetting(
                        "experiments",
                        withExperimentValue(
                          $settings.experiments,
                          exp.id,
                          e.currentTarget.value,
                        ),
                      )}
                  >
                    {#each exp.options as opt}
                      <option value={opt.value}>{opt.label}</option>
                    {/each}
                  </select>
                {/if}
              </div>
            {/each}
          {:else if selected === "advanced"}
            <div
              data-testid="runtime-debug-panel"
              class="mb-5 rounded-lg border border-border-subtle bg-bg-surface/30 p-3"
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <div class="text-[13px] font-semibold text-text-primary">Runtime</div>
                  <div class="mt-0.5 text-[11px] text-text-muted">
                    {#if runtimeStatus}
                      {runtimeModeLabel(runtimeStatus)}
                    {:else if runtimeStatusLoading}
                      Checking…
                    {:else}
                      Unavailable
                    {/if}
                  </div>
                </div>
                <button
                  aria-label="Refresh runtime status"
                  class="rounded border border-border px-2.5 py-1 text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-50"
                  disabled={runtimeStatusLoading}
                  onclick={() => {
                    const run = ++runtimeStatusRun;
                    void refreshRuntimeStatus(run);
                  }}
                >
                  Refresh
                </button>
              </div>

              {#if runtimeStatusError}
                <div class="mt-3 text-[11px] text-red">{runtimeStatusError}</div>
              {:else if runtimeStatus}
                <div class="mt-3 grid grid-cols-[100px_minmax(0,1fr)] gap-x-3 gap-y-1 text-[11px]">
                  <div class="text-text-muted">Mode</div>
                  <div class="text-text-primary">{runtimeModeLabel(runtimeStatus)}</div>

                  <div class="text-text-muted">Desktop PID</div>
                  <div class="font-mono text-text-secondary">pid {runtimeStatus.desktopPid}</div>

                  <div class="text-text-muted">Started</div>
                  <div class="text-text-secondary">{formatTimestamp(runtimeStatus.startedAtMs)}</div>

                  <div class="text-text-muted">Uptime</div>
                  <div class="text-text-secondary">{formatDuration(runtimeStatus.uptimeMs)}</div>

                  {#if runtimeStatus.daemon}
                    <div class="text-text-muted">Daemon PID</div>
                    <div class="font-mono text-text-secondary">pid {runtimeStatus.daemon.pid}</div>

                    <div class="text-text-muted">Socket</div>
                    <div class="break-all font-mono text-text-secondary">{runtimeStatus.daemon.socket}</div>

                    {#if runtimeStatus.daemon.logPath}
                      <div class="text-text-muted">Daemon log</div>
                      <div class="break-all font-mono text-text-secondary">{runtimeStatus.daemon.logPath}</div>
                    {/if}

                    <div class="text-text-muted">State</div>
                    <div class="text-text-secondary">
                      {runtimeStatus.daemon.sessionCount} sessions,
                      {runtimeStatus.daemon.projectCount} projects,
                      {runtimeStatus.daemon.watchCount ?? 0} watches,
                      {runtimeStatus.daemon.processCount ?? 0} processes,
                      {runtimeStatus.daemon.ptyCount ?? 0} PTYs
                    </div>
                  {:else if runtimeStatus.local}
                    <div class="text-text-muted">State</div>
                    <div class="text-text-secondary">
                      {runtimeStatus.local.sessionCount} sessions,
                      {runtimeStatus.local.projectCount} projects,
                      {runtimeStatus.local.watchCount} watches,
                      {runtimeStatus.local.processCount} processes,
                      {runtimeStatus.local.ptyCount} PTYs
                    </div>
                  {/if}

                  {#if runtimeStatus.statusError}
                    <div class="text-text-muted">Status error</div>
                    <div class="break-all text-amber">{runtimeStatus.statusError}</div>
                  {/if}
                </div>
              {/if}
            </div>

            <div class="flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Current version</div>
                <div class="text-[11px] text-text-muted mt-0.5">{appVersion}</div>
              </div>
              <button
                class="rounded border border-border px-2.5 py-1 text-[11px] text-text-primary hover:bg-bg-hover disabled:opacity-50"
                disabled={$updateStatus.kind === "checking" || $updateStatus.kind === "downloading"}
                onclick={() => void runManualCheck()}
              >
                {$updateStatus.kind === "checking" ? "Checking…" : "Check for updates"}
              </button>
            </div>

            {#if $updateStatus.kind === "no-update"}
              <div class="mt-2 text-[11px] text-text-secondary">You're on the latest version.</div>
            {:else if $updateStatus.kind === "available"}
              <div class="mt-3 rounded-lg border border-accent/30 bg-accent/5 p-3">
                <div class="text-[12px] font-semibold text-text-primary">Update available: {$updateStatus.version}</div>
                {#if $updateStatus.notes}
                  <pre class="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap text-[11px] text-text-secondary">{$updateStatus.notes}</pre>
                {/if}
                <button
                  class="mt-3 rounded border border-accent bg-accent-dim px-3 py-1 text-[11px] font-semibold text-text-primary hover:bg-accent/40"
                  onclick={() => void performInstall()}
                >
                  Install and restart
                </button>
              </div>
            {:else if $updateStatus.kind === "downloading"}
              <div class="mt-3 rounded-lg border border-border-subtle bg-bg-surface/35 p-3">
                <div class="text-[11px] text-text-secondary">
                  Downloading update{$updateStatus.progress !== null ? ` (${Math.round($updateStatus.progress * 100)}%)` : "…"}
                </div>
                <div class="mt-2 h-1.5 w-full overflow-hidden rounded bg-bg-deep">
                  <div
                    class="h-full bg-accent transition-[width] duration-200"
                    style="width: {$updateStatus.progress !== null ? Math.round($updateStatus.progress * 100) : 30}%"
                  ></div>
                </div>
              </div>
            {:else if $updateStatus.kind === "installed-restart-required"}
              <div class="mt-3 rounded-lg border border-accent/30 bg-accent/5 p-3">
                <div class="text-[12px] font-semibold text-text-primary">Update installed</div>
                <div class="text-[11px] text-text-secondary mt-0.5">Quit and reopen Roux to finish.</div>
                <button
                  class="mt-3 rounded border border-accent bg-accent-dim px-3 py-1 text-[11px] font-semibold text-text-primary hover:bg-accent/40"
                  onclick={() => void quitApp()}
                >
                  Quit Roux
                </button>
              </div>
            {:else if $updateStatus.kind === "error"}
              <div class="mt-2 text-[11px] text-red">{describeError($updateStatus.reason)}</div>
            {/if}

            <div class="mt-4 flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Update channel</div>
                <div class="text-[11px] text-text-muted mt-0.5">Switching to Stable takes effect on the next stable release at or above your current version.</div>
              </div>
              <select
                class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
                value={$settings.updateChannel ?? "stable"}
                onchange={(e) => updateSetting("updateChannel", e.currentTarget.value as UpdateChannel)}
              >
                <option value="stable">Stable</option>
                <option value="preRelease">Pre-release (Alpha)</option>
              </select>
            </div>

            <div class="mt-4 flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Check for updates on launch</div>
                <div class="text-[11px] text-text-muted mt-0.5">Silently check in the background when Roux starts</div>
              </div>
              <button
                aria-label="Toggle auto-check on launch"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {($settings.updateCheckOnLaunch ?? true) ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => updateSetting('updateCheckOnLaunch', !($settings.updateCheckOnLaunch ?? true))}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {($settings.updateCheckOnLaunch ?? true) ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>

            <div class="mt-4 flex items-center justify-between py-2">
              <div>
                <div class="text-[13px]">Enable logging</div>
                <div class="text-[11px] text-text-muted mt-0.5">Write logs to disk for debugging</div>
              </div>
              <button
                aria-label="Toggle logging"
                class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
                  {$settings.enableLogging ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
                onclick={() => {
                  const next = !$settings.enableLogging;
                  setLoggingEnabled(next);
                  updateSetting("enableLogging", next);
                }}
              >
                <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
                  {$settings.enableLogging ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
              </button>
            </div>
            {#if $settings.enableLogging}
              <div class="text-[11px] text-text-muted font-mono mt-1 break-all">{getLogPath()}</div>
            {/if}

            <div class="mt-6 border-t border-hairline pt-5">
              <DoctorPanel mode="settings" />
            </div>
          {/if}
        </div>
      </div>

      <!-- Drag-to-resize grip, anchored to the dialog's bottom-right corner. -->
      <div
        class="group absolute bottom-0 right-0 z-10 flex h-5 w-5 cursor-nwse-resize items-end justify-end p-1"
        role="separator"
        aria-label="Resize settings window"
        aria-orientation="horizontal"
        title="Drag to resize"
        onpointerdown={onResizePointerDown}
        onpointermove={onResizePointerMove}
        onpointerup={onResizePointerUp}
      >
        <span
          class="pointer-events-none h-2 w-2 border-b-2 border-r-2 border-text-muted/40 transition-colors group-hover:border-text-muted/80"
        ></span>
      </div>
    </div>
  </div>
{/if}
