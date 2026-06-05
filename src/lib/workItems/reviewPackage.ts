import type { WorkItem } from "$lib/bindings";
import type { Attachment, WorkItemRun, WorkItemRunEvent } from "$lib/types/workItems";
import { isPlanAttachment } from "$lib/workItems/planningGate";

export interface WorkItemReviewPackageAttachment {
  title: string;
  documentId: string;
}

export interface WorkItemReviewPackage {
  runId: string | null;
  sessionId: string | null;
  plan: WorkItemReviewPackageAttachment | null;
  feedback: WorkItemReviewPackageAttachment | null;
  agentSummary: string | null;
  tests: string | null;
  changedFiles: string[];
  worktreePath: string | null;
  worktreeLabel: string | null;
  branch: string | null;
  prUrl: string | null;
}

function compareUpdated(left: { updatedAt: number; createdAt: number; id: string }, right: {
  updatedAt: number;
  createdAt: number;
  id: string;
}): number {
  return (
    left.updatedAt - right.updatedAt ||
    left.createdAt - right.createdAt ||
    left.id.localeCompare(right.id)
  );
}

function latestByTimestamp<T extends { updatedAt: number; createdAt: number; id: string }>(
  values: T[],
): T | null {
  return values.reduce<T | null>((latest, value) => {
    if (!latest) return value;
    return compareUpdated(value, latest) >= 0 ? value : latest;
  }, null);
}

function attachmentLabel(attachment: Attachment): WorkItemReviewPackageAttachment {
  return {
    title: attachment.title?.trim() || attachment.documentId,
    documentId: attachment.documentId,
  };
}

function titleWords(value: string | null): Set<string> {
  return new Set(
    (value ?? "")
      .split(/[^a-zA-Z0-9]+/)
      .map((word) => word.toLowerCase())
      .filter(Boolean),
  );
}

function isReviewFeedbackAttachment(attachment: Attachment): boolean {
  if (attachment.targetKind !== "workItem") return false;
  const words = titleWords(attachment.title);
  return (
    words.has("feedback") && (words.has("review") || words.has("changes"))
  );
}

function latestImplementationReviewRun(runs: WorkItemRun[]): WorkItemRun | null {
  return latestByTimestamp(
    runs.filter(
      (run) => run.kind === "implementation" && run.status === "review",
    ),
  );
}

function latestImplementationRun(runs: WorkItemRun[]): WorkItemRun | null {
  return latestByTimestamp(runs.filter((run) => run.kind === "implementation"));
}

function pathLabel(path: string | null): string | null {
  if (!path) return null;
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts.slice(-2).join("/") || path;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function firstString(record: Record<string, unknown>, keys: string[]): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return null;
}

function stringsFrom(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((entry): entry is string => typeof entry === "string" && !!entry.trim());
}

function testsFrom(value: unknown): string | null {
  if (typeof value === "string" && value.trim()) return value.trim();
  const lines = stringsFrom(value);
  return lines.length > 0 ? lines.join("\n") : null;
}

function extractEventDetails(
  runId: string | null,
  events: WorkItemRunEvent[],
): Pick<WorkItemReviewPackage, "agentSummary" | "tests" | "changedFiles"> {
  let agentSummary: string | null = null;
  let tests: string | null = null;
  const changedFiles = new Set<string>();

  for (const event of events) {
    if (runId && event.runId !== runId) continue;
    const payload = asRecord(event.payload);
    if (!payload) continue;
    agentSummary ??= firstString(payload, ["agentSummary", "summary"]);
    tests ??= testsFrom(payload.tests);
    for (const file of [
      ...stringsFrom(payload.changedFiles),
      ...stringsFrom(payload.files),
    ]) {
      changedFiles.add(file);
    }
  }

  return { agentSummary, tests, changedFiles: [...changedFiles] };
}

export function buildWorkItemReviewPackage(
  item: WorkItem,
  runs: WorkItemRun[],
  attachments: Attachment[],
  events: WorkItemRunEvent[] = [],
): WorkItemReviewPackage {
  const run = latestImplementationReviewRun(runs) ?? latestImplementationRun(runs);
  const plan = latestByTimestamp(attachments.filter(isPlanAttachment));
  const feedback = latestByTimestamp(
    attachments.filter(isReviewFeedbackAttachment),
  );
  const worktreePath = run?.worktreePath ?? item.worktreePath;
  const eventDetails = extractEventDetails(run?.id ?? null, events);

  return {
    runId: run?.id ?? null,
    sessionId: run?.sessionId ?? item.sessionId,
    plan: plan ? attachmentLabel(plan) : null,
    feedback: feedback ? attachmentLabel(feedback) : null,
    agentSummary: eventDetails.agentSummary,
    tests: eventDetails.tests,
    changedFiles: eventDetails.changedFiles,
    worktreePath,
    worktreeLabel: pathLabel(worktreePath),
    branch: run?.branch ?? item.branch,
    prUrl: item.pinnedPrUrl ?? null,
  };
}
