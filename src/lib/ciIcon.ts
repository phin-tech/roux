// Maps a worktrunk CI status string to a lucide icon component and
// a Tailwind color class, so every surface that renders CI state
// (picker chips, session cards, status bar) stays consistent and uses
// the app's icon system instead of unicode glyphs.

import Check from "@lucide/svelte/icons/check";
import X from "@lucide/svelte/icons/x";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import LoaderCircle from "@lucide/svelte/icons/loader-circle";
import CircleAlert from "@lucide/svelte/icons/circle-alert";
import type { Component } from "svelte";

export type CiIcon = Component<{ size?: number; class?: string }>;

/**
 * Pick the icon + color for a `WorktrunkMetadata.ciStatus`. Returns
 * `null` when the status is absent or intentionally hidden (`"no-ci"`).
 */
export function ciChipFor(
  status: string | null | undefined,
): { icon: CiIcon; color: string; label: string } | null {
  switch (status) {
    case "passed":
      return { icon: Check, color: "text-green", label: "passed" };
    case "failed":
      return { icon: X, color: "text-red", label: "failed" };
    case "conflicts":
      return { icon: TriangleAlert, color: "text-red", label: "conflicts" };
    case "running":
      return { icon: LoaderCircle, color: "text-yellow", label: "running" };
    case "error":
      return { icon: CircleAlert, color: "text-red", label: "error" };
    case "no-ci":
    case null:
    case undefined:
    case "":
      return null;
    default:
      return { icon: CircleAlert, color: "text-text-muted", label: status };
  }
}
