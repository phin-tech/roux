// Tiny icon mappings for the PR checks + review-decision chips next to
// the status bar's PR link. Mirrors the shape of `ciIcon.ts` so every
// surface that needs these signals stays consistent.
//
// Deliberately small: the status bar is dense, and the icons are 12 px.
// We don't show counts inline — the tooltip does that.

import Check from "@lucide/svelte/icons/check";
import X from "@lucide/svelte/icons/x";
import LoaderCircle from "@lucide/svelte/icons/loader-circle";
import GitPullRequest from "@lucide/svelte/icons/git-pull-request";
import MessagesSquare from "@lucide/svelte/icons/messages-square";
import type { Component } from "svelte";

import type { PrChecksSummary } from "$lib/tauri";

export type PrChipIcon = Component<{ size?: number; class?: string }>;

export interface PrChipSpec {
  icon: PrChipIcon;
  /** Tailwind text-color class (e.g. `text-green`). */
  color: string;
  /** Short ARIA / hover label for the chip. */
  label: string;
  /** Spin the icon (used for `pending` / `running`). */
  spin: boolean;
}

/**
 * Map a `PrChecksSummary` to a chip. Returns `null` when the rollup is
 * absent or empty — there's nothing meaningful to show in that case.
 */
export function checksChipFor(
  checks: PrChecksSummary | null | undefined,
): PrChipSpec | null {
  if (!checks || checks.state === "none" || checks.total === 0) return null;
  switch (checks.state) {
    case "passing":
      return {
        icon: Check,
        color: "text-green",
        label: `${checks.passing}/${checks.total} checks passing`,
        spin: false,
      };
    case "failing":
      return {
        icon: X,
        color: "text-red",
        label: `${checks.failing}/${checks.total} checks failing`,
        spin: false,
      };
    case "pending":
      return {
        icon: LoaderCircle,
        color: "text-yellow",
        label: `${checks.pending}/${checks.total} checks pending`,
        spin: true,
      };
    default:
      return null;
  }
}

/**
 * Map GitHub's `reviewDecision` to a chip. Returns `null` for
 * `null` / unrecognized values so the chip simply doesn't render.
 *
 * `REVIEW_REQUIRED` is intentionally muted (gray) — it's a default
 * GitHub state for PRs that haven't been reviewed yet, not a problem
 * the user needs alerted to.
 */
export function reviewChipFor(
  decision: string | null | undefined,
): PrChipSpec | null {
  if (!decision) return null;
  switch (decision.toUpperCase()) {
    case "APPROVED":
      return {
        icon: GitPullRequest,
        color: "text-green",
        label: "Approved",
        spin: false,
      };
    case "CHANGES_REQUESTED":
      return {
        icon: MessagesSquare,
        color: "text-red",
        label: "Changes requested",
        spin: false,
      };
    case "REVIEW_REQUIRED":
      return {
        icon: GitPullRequest,
        color: "text-text-muted",
        label: "Review required",
        spin: false,
      };
    default:
      return null;
  }
}
