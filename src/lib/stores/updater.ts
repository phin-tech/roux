import { writable, get } from "svelte/store";
import {
  checkForUpdate,
  installUpdate,
  relaunchApp,
  type UpdateStatus,
} from "$lib/updater";
import { settings } from "./settings";

export const updateStatus = writable<UpdateStatus>({ kind: "idle" });

let startupCheckStarted = false;

export function runStartupCheck(): void {
  if (startupCheckStarted) return;
  startupCheckStarted = true;

  if (!(get(settings).updateCheckOnLaunch ?? true)) return;

  setTimeout(async () => {
    const status = await checkForUpdate({ silent: true });
    if (status.kind === "available" || status.kind === "error") {
      updateStatus.set(status);
    }
  }, 5000);
}

export async function runManualCheck(): Promise<void> {
  updateStatus.set({ kind: "checking" });
  const status = await checkForUpdate({ silent: false });
  updateStatus.set(status);
}

export async function performInstall(): Promise<void> {
  updateStatus.set({ kind: "downloading", progress: null });

  try {
    await installUpdate((progress) => {
      updateStatus.set({ kind: "downloading", progress });
    });
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    const reason = /signature/i.test(message)
      ? "signature-invalid"
      : /network|connect|dns|request|http|timeout/i.test(message)
        ? "network"
        : "unknown";
    updateStatus.set({ kind: "error", reason });
    return;
  }

  // Install succeeded. Show the "restart required" state first — if the
  // relaunch call succeeds the process exits and the user never sees this
  // state, but if relaunch throws (a known flake on macOS after the bundle
  // is swapped) we leave the message visible so the user knows to quit
  // and reopen Roux manually.
  updateStatus.set({ kind: "installed-restart-required" });

  try {
    await relaunchApp();
  } catch (e) {
    console.warn("[updater] relaunch failed after install; user must quit manually", e);
  }
}

export function dismissUpdateBanner(): void {
  updateStatus.set({ kind: "idle" });
}
