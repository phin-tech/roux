import { get } from "svelte/store";
import type { StatusUpdate } from "$lib/tauri";
import {
  updateAgentState,
  type AgentStateEvent,
  type CompletionSummary,
  type PermissionInfo,
} from "./agentState";
import { sessionLayouts, collectLeafIds } from "./layout";
import type { Provider } from "./profiles";
import { updateInstance } from "./instances";

/**
 * Cross-check that a given pane id belongs to the claimed session — the
 * callback form exists so unit tests can stub the lookup without pushing
 * Svelte stores through the test harness. The default implementation
 * walks `sessionLayouts` to find the leaf.
 */
export type PaneSessionCheck = (sessionId: string, paneId: string) => boolean;

function defaultPaneBelongsToSession(sessionId: string, paneId: string): boolean {
  const layout = get(sessionLayouts).get(sessionId);
  if (!layout) return false;
  for (const leafId of collectLeafIds(layout)) {
    if (leafId === paneId) return true;
  }
  return false;
}

/**
 * Outcome of routing a `roux-status-update` event.
 *
 * - `pane` means tier-1 routing fired — the event carried a `rouxPaneId`
 *   and we wrote to the pane-level `agentState` store. Downstream
 *   session-level updates should be skipped, because the session aggregate
 *   is derived from pane state.
 * - `legacy` means the hook install predates `ROUX_PANE_ID`, so we fell
 *   back to cwd matching. Per the spec these events should only drive
 *   notification fan-out, never pane-level state.
 * - `dropped` means the event wasn't actionable (unknown status, no
 *   provider context, etc.). The caller should log if interested and
 *   otherwise ignore it.
 */
export type StatusRouting =
  | { kind: "pane"; paneId: string; event: AgentStateEvent }
  | { kind: "legacy"; cwd: string; status: string }
  | { kind: "dropped"; reason: string };

/**
 * Classify an incoming `roux-status-update` event. Does not mutate any
 * store — `applyStatusRouting` takes the resulting decision and commits
 * it. Extracted so the decision logic can be unit-tested without spinning
 * up Svelte stores or Tauri listeners.
 *
 * `paneBelongsToSession` is overridable for tests; by default it consults
 * the live `sessionLayouts` store to verify the claimed pane id is an
 * actual leaf of the claimed session. The check prevents a hook payload
 * from smearing one session's aggregate status with another session's
 * pane state (either by planted file or by a restart/id-collision bug).
 */
export function routeStatusUpdate(
  update: StatusUpdate,
  paneBelongsToSession: PaneSessionCheck = defaultPaneBelongsToSession,
): StatusRouting {
  // Only tier-1 events know which specific pane the agent lives in. Legacy
  // events (cwd-only) still emit here so the legacy notification path keeps
  // working, but they must not touch pane-level state.
  if (!update.rouxPaneId) {
    return { kind: "legacy", cwd: update.cwd, status: update.status };
  }

  // Pane routing requires a session id so we can cross-check membership.
  // A payload carrying only `rouxPaneId` without `rouxSessionId` cannot
  // be validated, so we refuse rather than trusting it blindly.
  if (!update.rouxSessionId) {
    return {
      kind: "dropped",
      reason: `rouxPaneId "${update.rouxPaneId}" carries no rouxSessionId; refusing to route`,
    };
  }

  // Cross-session hijack guard: the claimed pane must live under the
  // claimed session in the live layout tree.
  if (!paneBelongsToSession(update.rouxSessionId, update.rouxPaneId)) {
    return {
      kind: "dropped",
      reason: `rouxPaneId "${update.rouxPaneId}" does not belong to rouxSessionId "${update.rouxSessionId}"`,
    };
  }

  const provider = inferProvider(update);
  if (!provider) {
    return {
      kind: "dropped",
      reason: `unrecognized provider for status "${update.status}"`,
    };
  }

  const routed = mapStatus(update.status);
  if (!routed) {
    // Non-routable statuses ("error", "disconnected") still fan out to
    // notifications via the legacy path; they don't move the pane's
    // agentState dot.
    return { kind: "dropped", reason: `non-routable status "${update.status}"` };
  }

  const permissionInfo = buildPermissionInfo(update);

  return {
    kind: "pane",
    paneId: update.rouxPaneId,
    event: {
      provider,
      status: routed,
      permissionInfo,
      completionSummary: buildCompletionSummary(update),
      providerSessionId: update.providerSessionId ?? undefined,
      source: "hook",
    },
  };
}

/**
 * Commit a routing decision against the runtime stores. Returns the same
 * decision it was given so callers can chain a `log`/`notify` on top.
 */
export function applyStatusRouting(routing: StatusRouting): StatusRouting {
  if (routing.kind === "pane") {
    updateAgentState(routing.paneId, routing.event);
    updateInstance(routing.paneId, {
      provider: routing.event.provider,
      providerSessionId: routing.event.providerSessionId,
    });
  }
  return routing;
}

function inferProvider(update: StatusUpdate): Provider | null {
  if (update.provider === "claude" || update.provider === "codex") {
    return update.provider;
  }
  // Legacy hook installs don't set `provider`. Until Codex support lands
  // broadly the only first-class provider in the wild is Claude, so treat
  // the unknown case as Claude rather than dropping the event — but only
  // when a provider session id is actually present. A bare status update
  // with neither provider nor session id shouldn't be silently coerced.
  if (update.providerSessionId && update.providerSessionId.length > 0) {
    return "claude";
  }
  return null;
}

function mapStatus(raw: string): "idle" | "generating" | null {
  switch (raw) {
    case "generating":
    case "thinking":
    case "attention":
      return "generating";
    case "idle":
      return "idle";
    default:
      return null;
  }
}

function buildPermissionInfo(update: StatusUpdate): PermissionInfo | undefined {
  const hasContent =
    update.status === "attention" ||
    !!update.toolName ||
    !!update.toolInput ||
    !!update.message;
  if (!hasContent) return undefined;
  return {
    toolName: update.toolName ?? undefined,
    toolInput: (update.toolInput as Record<string, unknown> | null) ?? undefined,
    message: update.message ?? undefined,
  };
}

function buildCompletionSummary(update: StatusUpdate): CompletionSummary | undefined {
  const query = update.query?.trim();
  const response = update.response?.trim();
  if (!query && !response) return undefined;
  return {
    query: query || undefined,
    response: response || undefined,
  };
}
