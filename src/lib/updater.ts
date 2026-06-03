import {
  commands,
  type UpdateChannel,
  type UpdaterError as BackendError,
} from "$lib/bindings";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";

type ProgressPayload =
  | { phase: "started"; contentLength: number | null }
  | { phase: "progress"; chunkLength: number }
  | { phase: "finished" };

export type UpdaterError = "network" | "signature-invalid" | "unknown";

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "no-update" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "downloading"; progress: number | null }
  | { kind: "installed-restart-required" }
  | { kind: "ready" }
  | { kind: "error"; reason: UpdaterError };

function isDev(): boolean {
  try {
    return Boolean(import.meta.env?.DEV);
  } catch {
    return false;
  }
}

function classifyBackendError(err: BackendError): UpdaterError {
  switch (err.kind) {
    case "signature-invalid":
      return "signature-invalid";
    case "network":
      return "network";
    default:
      return "unknown";
  }
}

function classifyTransportError(err: unknown): UpdaterError {
  const message = err instanceof Error ? err.message : String(err ?? "");
  if (/signature/i.test(message)) return "signature-invalid";
  if (/network|connect|dns|request|http|timeout/i.test(message))
    return "network";
  return "unknown";
}

export async function checkForUpdate(opts: {
  silent: boolean;
  channel: UpdateChannel;
}): Promise<UpdateStatus> {
  if (isDev()) {
    return { kind: "no-update" };
  }

  let reason: UpdaterError;
  let rawError: unknown;
  try {
    const result = await commands.checkForUpdate(opts.channel);
    if (result.status === "ok") {
      if (!result.data) return { kind: "no-update" };
      return {
        kind: "available",
        version: result.data.version,
        notes: result.data.notes ?? "",
      };
    }
    // "not-found" means the selected channel has no published release (e.g.
    // the pre-release channel before the first alpha ships). From the user's
    // perspective that's indistinguishable from "no update available".
    if (result.error.kind === "not-found") {
      return { kind: "no-update" };
    }
    reason = classifyBackendError(result.error);
    rawError = result.error;
  } catch (err) {
    reason = classifyTransportError(err);
    rawError = err;
  }

  // Signature errors are a tamper signal; always surface.
  if (reason === "signature-invalid") {
    return { kind: "error", reason };
  }
  if (opts.silent) {
    console.warn("[updater] check failed (silent):", reason, rawError);
    return { kind: "no-update" };
  }
  return { kind: "error", reason };
}

export async function installUpdate(
  opts: { channel: UpdateChannel },
  onProgress: (progress: number | null) => void,
): Promise<void> {
  if (isDev()) {
    throw new Error("installUpdate is not available in dev mode");
  }

  let contentLength: number | null = null;
  let downloaded = 0;
  let unlisten: UnlistenFn | null = null;

  try {
    unlisten = await listen<ProgressPayload>(
      "updater://progress",
      ({ payload }) => {
        switch (payload.phase) {
          case "started": {
            contentLength = payload.contentLength ?? null;
            downloaded = 0;
            onProgress(contentLength && contentLength > 0 ? 0 : null);
            break;
          }
          case "progress": {
            downloaded += payload.chunkLength;
            if (contentLength && contentLength > 0) {
              onProgress(downloaded / contentLength);
            } else {
              onProgress(null);
            }
            break;
          }
          case "finished": {
            onProgress(1);
            break;
          }
        }
      },
    );

    const result = await commands.installUpdate(opts.channel);
    if (result.status === "error") {
      const reason = classifyBackendError(result.error);
      // Mirror the legacy error shape so upstream callers match on .message.
      const err = new Error(reason);
      (err as Error & { reason?: UpdaterError }).reason = reason;
      throw err;
    }
  } finally {
    unlisten?.();
  }
  // Do NOT call relaunch() here. The install is complete; relaunch is a
  // separate, best-effort step handled by the caller so we can distinguish
  // "install failed" from "install succeeded but restart failed".
}

/**
 * Attempts to exit the current process and start a fresh one. May throw
 * on macOS if the bundle was just replaced on disk — callers should treat
 * any thrown error as "the install is fine, the user just needs to restart
 * Roux manually".
 */
export async function relaunchApp(): Promise<void> {
  if (isDev()) return;
  await relaunch();
}
