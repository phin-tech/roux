/**
 * Frontend type mirroring `roux_core::models::work_item::WorkItemEvent`.
 *
 * Hand-written rather than generated via specta: the event is delivered over
 * the "work-item-event" Tauri channel (see `onWorkItemEvent` in tauri.ts) and
 * is not referenced by any collected command, so `Builder::export` cannot
 * reach it and a regenerated `bindings.ts` would omit it. Keep in sync with
 * the Rust enum (serde tag `"type"`, `rename_all = "camelCase"`). `WorkItem`
 * and `WorkItemStatus` stay generated in `bindings.ts`.
 */
import type { WorkItem, WorkItemStatus } from "$lib/bindings";

export type WorkItemEvent =
  | { type: "created"; item: WorkItem }
  | { type: "updated"; item: WorkItem }
  | { type: "moved"; id: string; status: WorkItemStatus; sortOrder: number }
  | { type: "deleted"; id: string }
  | { type: "imported"; ids: string[] }
  | { type: "sessionBound"; id: string; sessionId: string };
