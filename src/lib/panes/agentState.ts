import { writable, derived, get, type Readable } from "svelte/store";
import type { Provider } from "$lib/bindings";
import { sessionLayouts, collectLeafIds, type LayoutNode } from "./layout";

/**
 * Observed runtime status for an agent running inside a shell pane.
 *
 * There is deliberately no `"disconnected"` state. PTY death is observable
 * at the pane level via the dead-pane view; agent process death without
 * shell death manifests as `agentState` sitting at its last value until the
 * pane closes or a later event supersedes it. A future pass can add OSC
 * 133 prompt-marker detection to auto-clear stale state on the next shell
 * prompt.
 */
export type AgentStatus = "idle" | "generating";

/**
 * Structural info from an attention-level hook payload. Used by the
 * Claude-specific Allow/Deny UI, which is gated on
 * `agentState?.provider === "claude" && permissionInfo != null`.
 */
export interface PermissionInfo {
  toolName?: string;
  toolInput?: Record<string, unknown> | null;
  message?: string;
}

/**
 * Runtime-only view of what an agent is doing in a particular pane. Never
 * persisted — entries are populated by incoming hook / OSC / `roux notify`
 * events and cleared on pane disposal or session close.
 */
export interface AgentState {
  provider: Provider;
  status: AgentStatus;
  permissionInfo?: PermissionInfo;
  providerSessionId?: string;
  /** Which event stream set this state last — useful for tracing. */
  source: "hook" | "osc" | "notify";
  /** `Date.now()` at last write. */
  updatedAt: number;
}

/**
 * Map from pane id → current observed agent state. A pane has no entry
 * until the first relevant event arrives; until then the pane is a plain
 * shell to the UI. Rendering `session-card status dot` / `pane header
 * badge` / provider-specific affordances all read from here.
 */
export const agentStates = writable<Map<string, AgentState>>(new Map());

/** Shape of an event to merge into a pane's `AgentState`. */
export interface AgentStateEvent {
  provider: Provider;
  status: AgentStatus;
  permissionInfo?: PermissionInfo;
  providerSessionId?: string;
  source: AgentState["source"];
}

/**
 * Upsert a pane's agent state. Merges `permissionInfo` and
 * `providerSessionId` onto the previous entry when the new event omits
 * them — keeps the Allow/Deny UI alive when a generating→idle tick lands
 * between permission updates.
 */
export function updateAgentState(paneId: string, event: AgentStateEvent): void {
  agentStates.update((map) => {
    const next = new Map(map);
    const prev = next.get(paneId);
    next.set(paneId, {
      provider: event.provider,
      status: event.status,
      permissionInfo: event.permissionInfo ?? prev?.permissionInfo,
      providerSessionId: event.providerSessionId ?? prev?.providerSessionId,
      source: event.source,
      updatedAt: Date.now(),
    });
    return next;
  });
}

/**
 * Clear the agent state entry for a pane. Called from pane disposal so the
 * session-card aggregate goes dark on close; *not* called on idle, since
 * "idle with a live agent" is a real state the user wants to see.
 */
export function disposeAgentState(paneId: string): void {
  agentStates.update((map) => {
    if (!map.has(paneId)) return map;
    const next = new Map(map);
    next.delete(paneId);
    return next;
  });
}

/**
 * Aggregate status for a session's sidebar card, derived from its shell
 * panes' agent states:
 * - `generating` when any pane is generating
 * - `idle` when at least one pane has an entry and none are generating
 * - `null` when no pane in the session has an agent-state entry
 */
export type AggregateStatus = "idle" | "generating";

function aggregateFor(
  layout: LayoutNode | undefined,
  states: Map<string, AgentState>,
): AggregateStatus | null {
  if (!layout) return null;
  let sawIdle = false;
  for (const paneId of collectLeafIds(layout)) {
    const s = states.get(paneId);
    if (!s) continue;
    if (s.status === "generating") return "generating";
    if (s.status === "idle") sawIdle = true;
  }
  return sawIdle ? "idle" : null;
}

/**
 * Derived session-level status map. Sidebar cards subscribe to this instead
 * of the legacy per-session `session.status` field (which is being phased
 * out; see spec phase 4/5).
 */
export const sessionAgentStatus: Readable<Map<string, AggregateStatus | null>> =
  derived(
    [sessionLayouts, agentStates],
    ([$layouts, $states]) => {
      const out = new Map<string, AggregateStatus | null>();
      for (const [sessionId, layout] of $layouts) {
        out.set(sessionId, aggregateFor(layout, $states));
      }
      return out;
    },
  );

/** Synchronous snapshot of the aggregate — handy for tests and one-shot queries. */
export function getSessionAgentStatus(
  sessionId: string,
): AggregateStatus | null {
  return aggregateFor(get(sessionLayouts).get(sessionId), get(agentStates));
}

/** Test-only reset hook. */
export function resetAgentStates(): void {
  agentStates.set(new Map());
}
