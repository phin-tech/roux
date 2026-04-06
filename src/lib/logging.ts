import { invoke } from "@tauri-apps/api/core";

let logPath: string | null = null;

/** Initialize frontend logging. Fetches log path from backend. */
export async function initLogging() {
  try {
    logPath = await invoke<string | null>("get_log_path");
    log("Frontend logging initialized");
  } catch {
    // Backend not ready yet, fall back to console only
  }
}

/** Log a message to both console and the backend log file. */
export function log(msg: string) {
  console.log(`[roux] ${msg}`);
  invoke("frontend_log", { message: msg }).catch(() => {});
}

/** Log an error to both console and the backend log file. */
export function logError(msg: string, err?: unknown) {
  const detail = err instanceof Error ? err.message : err ? String(err) : "";
  const full = detail ? `${msg}: ${detail}` : msg;
  console.error(`[roux] ${full}`);
  invoke("frontend_log", { message: `ERROR ${full}` }).catch(() => {});
}

/** Get the path to the log file (for display in settings/debug). */
export function getLogPath(): string | null {
  return logPath;
}
