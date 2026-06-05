import type { Attachment } from "$lib/types/workItems";

function normalize(value: string | null): string {
  return value?.trim().toLowerCase() ?? "";
}

function basename(path: string): string {
  const parts = path.replaceAll("\\", "/").split("/");
  return parts[parts.length - 1] ?? path;
}

function tokens(value: string): Set<string> {
  return new Set(value.split(/[^a-z0-9]+/).filter(Boolean));
}

function isPlanLabel(value: string): boolean {
  if (!value) return false;
  return tokens(value).has("plan");
}

export function isPlanAttachment(attachment: Attachment): boolean {
  if (attachment.targetKind !== "workItem") return false;
  if (isPlanLabel(normalize(attachment.title))) return true;

  const sourceName = basename(normalize(attachment.sourcePath));
  return isPlanLabel(sourceName);
}

export function hasAttachedPlan(attachments: Attachment[]): boolean {
  return attachments.some(isPlanAttachment);
}

export function canStartImplementationFromPlanning(
  attachments: Attachment[],
  force: boolean,
): boolean {
  return force || hasAttachedPlan(attachments);
}
