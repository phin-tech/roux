import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import type { Webview } from "@tauri-apps/api/webview";

export interface RetainedExternalToolWebview {
  runId: string;
  key: string;
  label: string;
  webview: Webview;
}

const retainedWebviews = new Map<string, RetainedExternalToolWebview>();

export function retainExternalToolWebview(
  entry: RetainedExternalToolWebview,
): void {
  closeRetainedExternalToolWebview(entry.runId);
  retainedWebviews.set(entry.runId, entry);
  hideNativeWebview(entry.webview);
}

export function takeRetainedExternalToolWebview(
  runId: string,
  key: string,
): RetainedExternalToolWebview | null {
  const retained = retainedWebviews.get(runId);
  if (!retained) return null;
  if (retained.key !== key) {
    closeRetainedExternalToolWebview(runId);
    return null;
  }
  retainedWebviews.delete(runId);
  return retained;
}

export function closeRetainedExternalToolWebview(runId: string): void {
  const retained = retainedWebviews.get(runId);
  if (!retained) return;
  retainedWebviews.delete(runId);
  closeNativeWebview(retained.webview);
}

export function closeNativeWebview(current: Webview | null): void {
  if (!current) return;
  hideNativeWebview(current);
  void current.close().catch(() => {});
}

function hideNativeWebview(current: Webview): void {
  void current.hide().catch(() => {});
  void current.setSize(new LogicalSize(1, 1)).catch(() => {});
  void current.setPosition(new LogicalPosition(-32000, -32000)).catch(() => {});
}
