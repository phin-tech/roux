import type { WorkItem, WorkItemRun } from "$lib/bindings";
import type { WorkItemReviewPackage } from "$lib/workItems/reviewPackage";

/**
 * Resolved open target for a work-item card's terminal action.
 *
 * Callers pass this to `openSessionById(target.sessionId, { ptyId: target.ptyId })`.
 */
export type WorkItemOpenTarget = {
  sessionId: string;
  ptyId?: string | null;
  runId?: string | null;
  kind: "planning" | "implementation" | "review";
  label: string;
};

/**
 * Find the latest run (by updatedAt desc, then createdAt desc, then id) that
 * matches a predicate.
 */
function latestRunBy(
  runs: WorkItemRun[],
  predicate: (run: WorkItemRun) => boolean,
): WorkItemRun | null {
  let latest: WorkItemRun | null = null;
  for (const run of runs) {
    if (!predicate(run)) continue;
    if (!latest) {
      latest = run;
      continue;
    }
    if (
      run.updatedAt > latest.updatedAt ||
      (run.updatedAt === latest.updatedAt &&
        run.createdAt > latest.createdAt) ||
      (run.updatedAt === latest.updatedAt &&
        run.createdAt === latest.createdAt &&
        run.id > latest.id)
    ) {
      latest = run;
    }
  }
  return latest;
}

/**
 * Resolve the best terminal open target for a work-item card.
 *
 * Pure function — no side effects, no async, no store access.
 *
 * Decision rules (first match wins):
 *
 * 1. **Planning card** → latest planning run with a `sessionId`. Includes
 *    terminal/archived planning runs so the card can reopen its session after
 *    pane close or app restart.
 *
 * 2. **Bound session** → `item.sessionId` directly. This is the normal path
 *    for implementation cards that have an active bound session.
 *
 * 3. **Review card** → `reviewPackage.sessionId` if present, otherwise the
 *    latest review-kind run with a `sessionId`.
 *
 * 4. **Fallback — implementation runs** → latest implementation run with a
 *    `sessionId`. Covers cards whose `item.sessionId` was cleared but still
 *    have run records pointing at their session.
 *
 * 5. **Last resort — any run** → latest run of any kind with a `sessionId`.
 *
 * Returns `null` when no session can be found anywhere.
 */
export function resolveWorkItemOpenTarget(
  item: WorkItem,
  runs: WorkItemRun[],
  reviewPackage: WorkItemReviewPackage | null,
): WorkItemOpenTarget | null {
  // ── 1. Planning card → latest planning run ──────────────────────────
  if (item.status === "planning") {
    const planningRun = latestRunBy(
      runs,
      (run) => run.kind === "planning" && !!run.sessionId,
    );
    if (planningRun?.sessionId) {
      return {
        sessionId: planningRun.sessionId,
        ptyId: planningRun.ptyId,
        runId: planningRun.id,
        kind: "planning",
        label: "Open planning terminal",
      };
    }
  }

  // ── 2. Bound session ────────────────────────────────────────────────
  if (item.sessionId) {
    return {
      sessionId: item.sessionId,
      ptyId: null,
      runId: null,
      kind: "implementation",
      label: "Open terminal",
    };
  }

  // ── 3. Review card → review package or review runs ──────────────────
  if (item.status === "review") {
    if (reviewPackage?.sessionId) {
      return {
        sessionId: reviewPackage.sessionId,
        ptyId: null,
        runId: reviewPackage.runId ?? undefined,
        kind: "review",
        label: "Open review terminal",
      };
    }
    const reviewRun = latestRunBy(
      runs,
      (run) => run.kind === "review" && !!run.sessionId,
    );
    if (reviewRun?.sessionId) {
      return {
        sessionId: reviewRun.sessionId,
        ptyId: reviewRun.ptyId,
        runId: reviewRun.id,
        kind: "review",
        label: "Open review terminal",
      };
    }
  }

  // ── 4. Fallback — latest implementation run with a sessionId ────────
  const implRun = latestRunBy(
    runs,
    (run) => run.kind === "implementation" && !!run.sessionId,
  );
  if (implRun?.sessionId) {
    return {
      sessionId: implRun.sessionId,
      ptyId: implRun.ptyId,
      runId: implRun.id,
      kind: "implementation",
      label: "Open terminal",
    };
  }

  // ── 5. Last resort — any run with a sessionId ───────────────────────
  const anyRun = latestRunBy(runs, (run) => !!run.sessionId);
  if (anyRun?.sessionId) {
    const kind: WorkItemOpenTarget["kind"] =
      anyRun.kind === "planning"
        ? "planning"
        : anyRun.kind === "review"
          ? "review"
          : "implementation";
    return {
      sessionId: anyRun.sessionId,
      ptyId: anyRun.ptyId,
      runId: anyRun.id,
      kind,
      label:
        kind === "planning"
          ? "Open planning terminal"
          : kind === "review"
            ? "Open review terminal"
            : "Open terminal",
    };
  }

  return null;
}
