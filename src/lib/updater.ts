import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdaterError = "network" | "signature-invalid" | "unknown";

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "no-update" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "downloading"; progress: number | null }
  | { kind: "ready" }
  | { kind: "error"; reason: UpdaterError };

function isDev(): boolean {
  try {
    return Boolean(import.meta.env?.DEV);
  } catch {
    return false;
  }
}

function classifyError(err: unknown): UpdaterError {
  const message = err instanceof Error ? err.message : String(err ?? "");
  if (/signature/i.test(message)) {
    return "signature-invalid";
  }
  if (/network|connect|dns|request|http|timeout/i.test(message)) {
    return "network";
  }
  return "unknown";
}

export async function checkForUpdate(opts: {
  silent: boolean;
}): Promise<UpdateStatus> {
  if (isDev()) {
    return { kind: "no-update" };
  }

  try {
    const update = await check();
    if (!update) {
      return { kind: "no-update" };
    }
    return {
      kind: "available",
      version: update.version,
      notes: update.body ?? "",
    };
  } catch (err) {
    const reason = classifyError(err);
    if (reason === "signature-invalid") {
      // Signature errors are a tamper signal; always surface.
      return { kind: "error", reason };
    }
    if (opts.silent) {
      console.warn("[updater] check failed (silent):", err);
      return { kind: "no-update" };
    }
    return { kind: "error", reason };
  }
}

export async function installUpdate(
  onProgress: (progress: number | null) => void,
): Promise<void> {
  if (isDev()) {
    throw new Error("installUpdate is not available in dev mode");
  }

  const update = await check();
  if (!update) {
    throw new Error("No update available");
  }

  let contentLength: number | undefined;
  let downloaded = 0;

  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started": {
        contentLength = event.data.contentLength;
        downloaded = 0;
        onProgress(contentLength ? 0 : null);
        break;
      }
      case "Progress": {
        downloaded += event.data.chunkLength;
        if (contentLength && contentLength > 0) {
          onProgress(downloaded / contentLength);
        } else {
          onProgress(null);
        }
        break;
      }
      case "Finished": {
        onProgress(1);
        break;
      }
    }
  });

  await relaunch();
}
