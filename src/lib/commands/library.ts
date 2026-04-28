import { get } from "svelte/store";
import { registry } from "./registry";
import type { CommandItem } from "./registry";
import {
  librarySkillSyncRun,
  librarySkillSyncUnsync,
  listLibraryItems,
  notificationsPush,
  readLibraryItem,
  renderLibraryPrompt,
  writeToSession,
  type LibraryItem,
  type SkillSyncRunReport,
  type UnsyncReport,
} from "$lib/tauri";
import { activeSession } from "$lib/stores/sessions";
import { focusedPaneId } from "$lib/panes/focus";
import { getAttachedPtyId, paneInstances } from "$lib/panes/instances";
import { openLibraryWindow } from "$lib/stores/libraryWindow";
import { requestLibraryVariables } from "$lib/stores/libraryVariablePrompt";
import { logError } from "$lib/logging";

type LibraryDestination = "activePane" | "clipboard";
type LibrarySearchKind = "prompt" | "skill";

function sessionId(): string | null {
  return get(activeSession)?.id ?? null;
}

function targetPtyId(): string | null {
  const focused = get(focusedPaneId);
  if (focused) {
    const pane = get(paneInstances).get(focused);
    const ptyId = pane ? getAttachedPtyId(pane) : null;
    if (ptyId) return ptyId;
  }
  return sessionId();
}

function layerLabel(item: LibraryItem): string {
  if (item.sourceLayer === "activeRepo") return "active";
  if (item.sourceLayer === "localRepo" || item.sourceLayer === "gitRepo") return item.sourceLabel;
  return "global";
}

async function renderLibraryItem(item: LibraryItem): Promise<string | null> {
  if (item.itemType === "skill") {
    return (await readLibraryItem(item.id, sessionId())).body;
  }
  const variables = await requestLibraryVariables({
    title: item.title,
    variables: item.variables,
  });
  if (!variables) return null;
  return (await renderLibraryPrompt({
    itemId: item.id,
    sessionId: sessionId(),
    variables,
  })).content;
}

async function sendLibraryItem(item: LibraryItem, destination: LibraryDestination): Promise<void> {
  const content = await renderLibraryItem(item);
  if (content === null) return;

  if (destination === "clipboard") {
    await navigator.clipboard.writeText(content);
    await notify("Copied Library item", item.title, "success");
    return;
  }

  const ptyId = targetPtyId();
  if (!ptyId) {
    await notify("No active terminal", "Focus a terminal pane before sending a Library item.", "warning");
    return;
  }
  await writeToSession(ptyId, `${content}\r`);
  await notify("Sent Library item", item.title, "success");
}

async function notify(title: string, body: string, level: "success" | "warning" | "error" = "success"): Promise<void> {
  try {
    await notificationsPush({
      level,
      source: { type: "internal" },
      title,
      subtitle: null,
      body,
      sessionId: sessionId(),
      actions: [],
      dedupKey: `library-command:${title}:${body}`,
    });
  } catch (error) {
    logError(`library command notification failed: ${title}`, error);
  }
}

async function libraryItems(
  destination: LibraryDestination,
  itemType: LibrarySearchKind,
): Promise<CommandItem[]> {
  const items = await listLibraryItems(sessionId());
  return items.filter((item) => item.itemType === itemType).map((item) => ({
    id: `library.${destination}.${itemType}.${item.id}`,
    label: item.title,
    description: `${item.id} · ${layerLabel(item)}`,
    action: async () => {
      try {
        await sendLibraryItem(item, destination);
      } catch (error) {
        logError(`library.${destination} failed`, error);
        await notify(
          destination === "clipboard" ? "Copy Library item failed" : "Send Library item failed",
          error instanceof Error ? error.message : String(error),
          "error",
        );
      }
    },
  }));
}

function summarizeSyncReport(report: SkillSyncRunReport): {
  title: string;
  body: string;
  level: "success" | "warning";
} {
  const synced = report.results.filter(
    (r) => r.outcome === "synced" || r.outcome === "syncedAsCopyFallback",
  ).length;
  const skipped = report.results.filter((r) => r.outcome === "skipped").length;
  const failed = report.results.filter((r) => r.outcome === "failed").length;

  const parts: string[] = [];
  if (synced) parts.push(`Synced ${synced}`);
  if (skipped) parts.push(`skipped ${skipped} (conflicts)`);
  if (failed) parts.push(`${failed} failed`);
  if (report.stale.length) parts.push(`${report.stale.length} stale`);
  if (report.symlinkFallbackCount) {
    parts.push(`${report.symlinkFallbackCount} fell back to copy (OS denied symlink)`);
  }

  const level: "success" | "warning" =
    failed > 0 || skipped > 0 || report.symlinkFallbackCount > 0 ? "warning" : "success";
  const body = parts.length > 0 ? parts.join(" · ") : "Nothing to sync.";
  return {
    title: synced > 0 ? "Skills synced" : "Skill sync complete",
    body,
    level,
  };
}

function summarizeUnsyncReport(report: UnsyncReport): {
  title: string;
  body: string;
  level: "success" | "warning";
} {
  const deleted = report.results.filter((r) => r.outcome === "deleted").length;
  const kept = report.results.filter((r) => r.outcome === "keptDueToDrift").length;
  const failed = report.results.filter((r) => r.outcome === "failed").length;
  const parts: string[] = [];
  if (deleted) parts.push(`Removed ${deleted}`);
  if (kept) parts.push(`kept ${kept} (locally edited)`);
  if (failed) parts.push(`${failed} failed`);
  const level: "success" | "warning" = kept > 0 || failed > 0 ? "warning" : "success";
  return {
    title: deleted > 0 ? "Skills unsynced" : "Nothing to unsync",
    body: parts.length > 0 ? parts.join(" · ") : "No matching entries.",
    level,
  };
}

export function registerLibraryCommands() {
  registry.register({
    id: "library.search-prompts",
    label: "Search Library Prompts",
    category: "Library",
    getItems: () => libraryItems("activePane", "prompt"),
    inputPlaceholder: "Search prompts...",
  });

  registry.register({
    id: "library.search-skills",
    label: "Search Library Skills",
    category: "Library",
    getItems: () => libraryItems("activePane", "skill"),
    inputPlaceholder: "Search skills...",
  });

  registry.register({
    id: "library.copy-prompt-to-clipboard",
    label: "Copy Library Prompt to Clipboard",
    category: "Library",
    getItems: () => libraryItems("clipboard", "prompt"),
    inputPlaceholder: "Search prompts...",
  });

  registry.register({
    id: "library.copy-skill-to-clipboard",
    label: "Copy Library Skill to Clipboard",
    category: "Library",
    getItems: () => libraryItems("clipboard", "skill"),
    inputPlaceholder: "Search skills...",
  });

  registry.register({
    id: "library.open-manager",
    label: "Open Library Manager",
    category: "Library",
    execute: openLibraryWindow,
  });

  registry.register({
    id: "library.skills.sync",
    label: "Sync Library Skills",
    category: "Library",
    execute: async () => {
      try {
        const report = await librarySkillSyncRun(sessionId());
        const summary = summarizeSyncReport(report);
        await notify(summary.title, summary.body, summary.level);
      } catch (error) {
        logError("library.skills.sync failed", error);
        await notify(
          "Skill sync failed",
          error instanceof Error ? error.message : String(error),
          "error",
        );
      }
    },
  });

  registry.register({
    id: "library.skills.unsync-all",
    label: "Unsync All Library Skills",
    category: "Library",
    execute: async () => {
      try {
        const report = await librarySkillSyncUnsync({ type: "all" });
        const summary = summarizeUnsyncReport(report);
        await notify(summary.title, summary.body, summary.level);
      } catch (error) {
        logError("library.skills.unsync-all failed", error);
        await notify(
          "Unsync failed",
          error instanceof Error ? error.message : String(error),
          "error",
        );
      }
    },
  });
}
