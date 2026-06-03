import type { PaneInstance } from "./instances";
import type { MultiLineTarget } from "$lib/stores/multiLineEditor";
import type { Session, WorktrunkMetadata } from "$lib/types";

export type MultiLineContextChipKind =
  | "target"
  | "cwd"
  | "branch"
  | "git-state"
  | "profile";

export type MultiLineContextChipTone = "muted" | "accent" | "warn";

export interface MultiLineContextChip {
  kind: MultiLineContextChipKind;
  label: string;
  title: string;
  tone: MultiLineContextChipTone;
}

interface BuildContextChipOptions {
  pane: PaneInstance | null;
  session: Session | null;
  target: MultiLineTarget;
  metadata: WorktrunkMetadata | null;
  profileName: string | null;
}

function basename(path: string): string {
  const normalized = path.replace(/[\\/]+$/g, "");
  if (!normalized) return path === "/" ? "/" : "";
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? normalized;
}

function gitStateLabel(metadata: WorktrunkMetadata): string | null {
  const parts: string[] = [];
  if (metadata.dirty) parts.push("dirty");
  if (metadata.ahead > 0) parts.push(`↑${metadata.ahead}`);
  if (metadata.behind > 0) parts.push(`↓${metadata.behind}`);
  if (metadata.locked) parts.push("locked");
  return parts.length > 0 ? parts.join(" ") : null;
}

export function buildMultiLineEditorContextChips(
  opts: BuildContextChipOptions,
): MultiLineContextChip[] {
  const chips: MultiLineContextChip[] = [
    {
      kind: "target",
      label: opts.target,
      title: `Input target: ${opts.target}`,
      tone: opts.target === "claude" ? "accent" : "muted",
    },
  ];

  const cwd =
    opts.pane?.workingDir ||
    opts.session?.worktreePath ||
    opts.session?.repoRoot ||
    "";
  if (cwd) {
    chips.push({
      kind: "cwd",
      label: basename(cwd) || cwd,
      title: cwd,
      tone: "muted",
    });
  }

  if (opts.session?.isGitRepo && opts.session.branch) {
    chips.push({
      kind: "branch",
      label: opts.session.branch,
      title: `Git branch: ${opts.session.branch}`,
      tone: "muted",
    });

    if (opts.metadata) {
      const label = gitStateLabel(opts.metadata);
      if (label) {
        chips.push({
          kind: "git-state",
          label,
          title: [
            opts.metadata.dirty ? "Uncommitted changes" : null,
            opts.metadata.ahead > 0 ? `${opts.metadata.ahead} ahead` : null,
            opts.metadata.behind > 0 ? `${opts.metadata.behind} behind` : null,
            opts.metadata.locked ? "Locked" : null,
          ]
            .filter(Boolean)
            .join(", "),
          tone: opts.metadata.dirty || opts.metadata.locked ? "warn" : "muted",
        });
      }
    }
  }

  if (opts.profileName) {
    chips.push({
      kind: "profile",
      label: opts.profileName,
      title: `Profile: ${opts.profileName}`,
      tone: "muted",
    });
  }

  return chips;
}
