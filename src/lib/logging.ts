import { invoke } from "@tauri-apps/api/core";

let enabled = false;
let logPath = "";

/** Initialize frontend logging. Fetches log path from backend. */
export async function initLogging(loggingEnabled: boolean) {
  enabled = loggingEnabled;
  try {
    logPath = await invoke<string>("get_log_path");
    if (enabled) log("Frontend logging initialized");
  } catch {
    // Backend not ready yet
  }
}

/** Update whether logging is enabled (called when settings change). */
export function setLoggingEnabled(value: boolean) {
  enabled = value;
}

/** Log a message to both console and the backend log file. */
export function log(msg: string) {
  if (!enabled) return;
  console.log(`[roux] ${msg}`);
  invoke("frontend_log", { message: msg }).catch(() => {});
}

/** Log an error to both console and the backend log file. */
export function logError(msg: string, err?: unknown) {
  if (!enabled) return;
  const detail = err instanceof Error ? err.message : err ? String(err) : "";
  const full = detail ? `${msg}: ${detail}` : msg;
  console.error(`[roux] ${full}`);
  invoke("frontend_log", { message: `ERROR ${full}` }).catch(() => {});
}

/** Get the path to the log file (for display in settings). */
export function getLogPath(): string {
  return logPath;
}
