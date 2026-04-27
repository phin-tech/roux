import { writable, derived, get, type Readable } from "svelte/store";
import type { Provider, SessionStatus } from "$lib/bindings";
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

export interface CompletionSummary {
  query?: string;
  response?: string;
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
  completionSummary?: CompletionSummary;
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
  completionSummary?: CompletionSummary;
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
      completionSummary: event.status === "idle" ? event.completionSummary : undefined,
      providerSessionId: event.providerSessionId ?? prev?.providerSessionId,
      source: event.source,
      updatedAt: Date.now(),
    });
    return next;
  });
}

/**
 * Clear a pane's `permissionInfo` without touching its status or other
 * fields. Called when the backend FSM reports the pane has exited
 * `Attention` (user answered, agent died, etc.) via the
 * `agent-attention-cleared` event.
 *
 * This is deliberately separate from `updateAgentState`: an ordinary
 * status hook that arrives *while the agent is still waiting* should
 * preserve `permissionInfo` (the `?? prev?.permissionInfo` merge on
 * line 74 exists for that reason — intermediate idle/generating ticks
 * between attention events must not flicker the Allow/Deny UI). Only
 * the FSM's confirmed `Exit(Attention)` transition is authoritative
 * enough to actually clear.
 */
export function clearPermissionInfo(paneId: string): void {
  agentStates.update((map) => {
    const prev = map.get(paneId);
    if (!prev || prev.permissionInfo === undefined) return map;
    const next = new Map(map);
    next.set(paneId, { ...prev, permissionInfo: undefined, updatedAt: Date.now() });
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

/**
 * Combine a session's legacy `Session.status` with its per-pane
 * `sessionAgentStatus` aggregate into a single "what is this session
 * doing right now?" value for every UI consumer that needs to render a
 * single indicator (sidebar card dot, reconnect-gate, close-confirm
 * prompt, etc.).
 *
 * Precedence:
 *
 * 1. `"disconnected"` and `"error"` come from the backend session
 *    record and win unconditionally — if the primary PTY is gone
 *    there is nothing generating, and an agent-state entry that
 *    predates the disconnect must not silently repaint the card.
 * 2. A live agent aggregate (`"generating"` / `"idle"`) overrides the
 *    legacy field, so a pane actively generating always pulses even
 *    when `Session.status` is stuck at a stale "idle" from an earlier
 *    event we never bothered to clear.
 * 3. Otherwise the legacy field passes through.
 *
 * This is the intended unification path: the backend keeps owning
 * session-level liveness (connect/disconnect/error), the frontend owns
 * per-pane agent liveness via hooks, and this helper is the one spot
 * that decides the precedence between them. Consumers should never
 * reach past it to `Session.status` directly.
 */
export function computeEffectiveSessionStatus(
  rawSessionStatus: SessionStatus,
  agentAggregate: AggregateStatus | null,
): SessionStatus {
  if (rawSessionStatus === "disconnected") return "disconnected";
  if (rawSessionStatus === "error") return "error";
  if (agentAggregate === "generating") return "generating";
  if (agentAggregate === "idle") return "idle";
  return rawSessionStatus;
}

/** Test-only reset hook. */
export function resetAgentStates(): void {
  agentStates.set(new Map());
}
