/**
 * Frontend types mirroring `roux_core::event` and `roux_core::alias`.
 *
 * Hand-written rather than generated via specta because `Event.structured`
 * is `serde_json::Value`, which specta can't render as valid TypeScript
 * (same constraint as `pane_state` commands). Keep the field names
 * `serde(rename_all = "camelCase")` matches what Rust emits.
 */

export type EventKind = "task" | "result" | "question" | "fyi" | "signal";

export interface Event {
  id: string;
  createdAt: number;
  to: string | null;
  topic: string | null;
  from: string | null;
  kind: EventKind;
  correlationId: string | null;
  projectId: string | null;
  subject: string | null;
  body: string;
  /** Free-form structured payload. May be any JSON value. */
  structured: unknown | null;
}

export interface ReadState {
  eventId: string;
  recipient: string;
  readAt: number | null;
  ackedAt: number | null;
  ackResult: string | null;
}

export interface AgentAlias {
  alias: string;
  /** Cached parent session id (Phase 1 / legacy session-only bindings). */
  sessionId: string | null;
  /** Phase-1.5 canonical addressable target. When null, the alias falls
   *  back to the session's primary pane at delivery time. */
  paneId: string | null;
  projectId: string | null;
  /** True when the alias was auto-derived from the pane's name (vs an
   *  explicit `roux alias claim`). UI surfaces this with a lighter
   *  outline on the badge so the user can tell it's automatic. */
  autoClaimed: boolean;
  createdAt: number;
  updatedAt: number;
}

/**
 * Frontend mirror of `roux_core::MailboxEvent`. Tagged with `kind` matching
 * the Rust `#[serde(tag = "kind")]` annotation. Emitted on the
 * `mailbox-event` Tauri event channel for every store mutation.
 */
export type MailboxEvent =
  | { kind: "posted"; event: Event }
  | { kind: "read"; eventId: string; recipient: string }
  | {
      kind: "acked";
      eventId: string;
      recipient: string;
      result: string | null;
    }
  | { kind: "cleared"; recipient: string; count: number };

/**
 * Frontend mirror of `roux_core::AliasEvent`. Tagged with `kind`.
 */
export type AliasEvent =
  | { kind: "set"; alias: AgentAlias }
  | { kind: "unset"; canonical: string; projectId: string | null };
