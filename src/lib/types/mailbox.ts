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
  /** Set when the sender retracted the event. Recipients hide it from
   *  inbox views; the sender's `mailbox sent` view still surfaces it
   *  with this timestamp. */
  retractedAt: number | null;
}

export interface ReadState {
  eventId: string;
  recipient: string;
  readAt: number | null;
  ackedAt: number | null;
  ackResult: string | null;
}

/** A pane participating in a group alias. */
export interface AliasMember {
  paneId: string;
  joinedAt: number;
}

/** How a group alias distributes events.
 *  - `competingConsumer` (V1 default): first member to ack claims the
 *    event; others stop seeing it. Work-queue.
 *  - `broadcast`: declared for forward-compat. V1 currently behaves
 *    like `competingConsumer` until per-member ReadState ships. */
export type ConsumptionMode = "competingConsumer" | "broadcast";

export interface AgentAlias {
  alias: string;
  /** Cached parent session id (Phase 1 / legacy session-only bindings). */
  sessionId: string | null;
  /** Phase-1.5 canonical addressable target. When null, the alias falls
   *  back to the session's primary pane at delivery time. */
  paneId: string | null;
  projectId: string | null;
  /** Group members. Empty when this is a single-pane alias. */
  members: AliasMember[];
  consumptionMode: ConsumptionMode;
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
  | { kind: "cleared"; recipient: string; count: number }
  | {
      /** Bus subscription matched a published topic event. The event itself
       *  also fires as `posted`; this variant adds the per-subscriber
       *  delivery context so the UI can bump the subscriber's unread count
       *  and surface the delivery without a new mailbox row. */
      kind: "topicDelivered";
      eventId: string;
      recipient: string;
      subscriptionId: string;
    }
  | {
      /** Sender unsent the event. UIs should drop it from inbox views;
       *  the sender's "sent" view should mark it as retracted. */
      kind: "retracted";
      eventId: string;
    }
  | {
      /** Recipient dismissed a single event from their inbox view.
       *  Other recipients are unaffected. */
      kind: "dismissed";
      eventId: string;
      recipient: string;
    };

/**
 * Frontend mirror of `roux_core::AliasEvent`. Tagged with `kind`.
 */
export type AliasEvent =
  | { kind: "set"; alias: AgentAlias }
  | { kind: "unset"; canonical: string; projectId: string | null };

/**
 * Frontend mirror of `roux_core::BusSubscription`.
 */
export interface BusSubscription {
  id: string;
  alias: string;
  pattern: string;
  projectId: string | null;
  createdAt: number;
}

/**
 * Frontend mirror of `roux_core::BusSubscriptionEvent`. Tagged with `kind`.
 * Emitted on the `subscription-event` Tauri channel for every mutation.
 */
export type BusSubscriptionEvent =
  | { kind: "created"; subscription: BusSubscription }
  | { kind: "removed"; id: string };
