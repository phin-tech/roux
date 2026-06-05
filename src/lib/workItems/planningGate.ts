import type { Attachment } from "$lib/types/workItems";

function normalize(value: string | null): string {
  return value?.trim().toLowerCase() ?? "";
}

function basename(path: string): string {
  const parts = path.replaceAll("\\", "/").split("/");
  return parts[parts.length - 1] ?? path;
}

function isPlanLabel(value: string): boolean {
  if (!value) return false;
  return value === "plan" || /\bplan\b/.test(value);
}

function isMarkdownAttachment(attachment: Attachment): boolean {
  const mimeType = normalize(attachment.mimeType);
  const sourcePath = normalize(attachment.sourcePath);
  return (
    mimeType === "text/markdown" ||
    sourcePath.endsWith(".md") ||
    sourcePath.endsWith(".markdown")
  );
}

export function isPlanAttachment(attachment: Attachment): boolean {
  if (attachment.targetKind !== "workItem") return false;
  if (isPlanLabel(normalize(attachment.title))) return true;

  const sourceName = basename(normalize(attachment.sourcePath));
  return isMarkdownAttachment(attachment) && isPlanLabel(sourceName);
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
