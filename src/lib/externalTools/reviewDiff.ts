import type { WorkItemReviewPackage } from "$lib/workItems/reviewPackage";
import type { ReviewContext } from "$lib/tauri";
import { openExternalToolForSession } from "$lib/stores/externalTools";

export function buildReviewDiffContext(
  reviewPackage: WorkItemReviewPackage,
): ReviewContext {
  return {
    base: null,
    changedFiles: reviewPackage.changedFiles,
  };
}

export async function launchReviewDiff(
  toolId: string,
  sessionId: string,
  reviewPackage: WorkItemReviewPackage,
): Promise<void> {
  const review = buildReviewDiffContext(reviewPackage);
  await openExternalToolForSession(toolId, sessionId, review);
}
