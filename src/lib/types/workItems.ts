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

export type WorkItemRunStatus =
  | "queued"
  | "starting"
  | "running"
  | "blocked"
  | "review"
  | "changesRequested"
  | "failed"
  | "stopped"
  | "done";

export type WorkItemRunKind = "planning" | "implementation" | "review";

export interface WorkItemRun {
  id: string;
  workItemId: string;
  kind: WorkItemRunKind;
  sessionId: string | null;
  ptyId: string | null;
  provider: string | null;
  profileId: string | null;
  status: WorkItemRunStatus;
  worktreePath: string | null;
  branch: string | null;
  cost: number | null;
  createdAt: number;
  startedAt: number | null;
  endedAt: number | null;
  updatedAt: number;
}

export type WorkItemRunEventKind =
  | "lifecycle"
  | "text"
  | "toolUse"
  | "toolResult"
  | "decision"
  | "decisionResolved"
  | "decisionTimedOut"
  | "result"
  | "error"
  | "statusChanged";

export interface WorkItemRunEvent {
  id: string;
  runId: string;
  kind: WorkItemRunEventKind;
  payload: unknown;
  createdAt: number;
}

export interface WorkItemDecisionOption {
  value: string;
  label: string;
}

export type WorkItemDecisionStatus = "pending" | "resolved" | "timedOut";

export interface WorkItemDecision {
  id: string;
  runId: string;
  question: string;
  options: WorkItemDecisionOption[];
  defaultValue: string | null;
  timeoutAt: number | null;
  status: WorkItemDecisionStatus;
  resolvedValue: string | null;
  resolvedBy: string | null;
  createdAt: number;
  resolvedAt: number | null;
  updatedAt: number;
}

export type AttachmentTargetKind = "session" | "workItem";
export type AttachmentContentKind = "text" | "file";

export interface Attachment {
  id: string;
  documentId: string;
  targetKind: AttachmentTargetKind;
  targetId: string;
  title: string | null;
  contentKind: AttachmentContentKind;
  mimeType: string | null;
  sourcePath: string | null;
  byteLen: number;
  sha256: string;
  createdAt: number;
  updatedAt: number;
}

export interface AttachmentDocument {
  attachment: Attachment;
  content: string;
}

export interface AttachmentInput {
  targetKind: AttachmentTargetKind;
  targetId: string;
  title?: string | null;
  contentKind: AttachmentContentKind;
  content: string;
  mimeType?: string | null;
  sourcePath?: string | null;
}

export type WorkItemEvent =
  | { type: "created"; item: WorkItem }
  | { type: "updated"; item: WorkItem }
  | { type: "moved"; id: string; status: WorkItemStatus; sortOrder: number }
  | { type: "deleted"; id: string }
  | { type: "imported"; ids: string[] }
  | { type: "documentAttached"; attachment: Attachment }
  | { type: "sessionBound"; id: string; sessionId: string }
  | { type: "runCreated"; run: WorkItemRun }
  | { type: "runUpdated"; run: WorkItemRun }
  | { type: "runEventAppended"; event: WorkItemRunEvent }
  | { type: "decisionCreated"; decision: WorkItemDecision }
  | { type: "decisionResolved"; decision: WorkItemDecision }
  | { type: "decisionTimedOut"; decision: WorkItemDecision };
