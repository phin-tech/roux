import type { Notification } from "../bindings";

/**
 * Frontend mirror of `roux_core::NotificationEvent`. The Rust enum is emitted
 * as an internally-tagged event and serialized with `#[serde(tag = "type")]`,
 * so the wire format carries `type` + the variant fields.
 */
export type NotificationEvent =
  | { type: "added"; notification: Notification }
  | { type: "updated"; notification: Notification }
  | { type: "read"; id: string }
  | { type: "readAll"; sessionId: string | null }
  | { type: "removed"; id: string }
  | { type: "cleared"; sessionId: string | null };
