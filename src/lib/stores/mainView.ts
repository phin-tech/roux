import { derived, get, writable } from "svelte/store";

export type MainViewRoute =
  | { kind: "board" }
  | { kind: "sessionDetail"; sessionId: string }
  | { kind: "externalTool"; runId: string }
  | { kind: "preferences"; category?: string; externalToolId?: string | null };

export const mainViewRoute = writable<MainViewRoute | null>(null);
export const mainViewActive = derived(mainViewRoute, ($route) => $route !== null);

export function openMainView(route: MainViewRoute): void {
  mainViewRoute.set(route);
}

export function closeMainView(): void {
  mainViewRoute.set(null);
}

export function toggleMainView(route: MainViewRoute): void {
  const current = get(mainViewRoute);
  if (current && routesEqual(current, route)) {
    closeMainView();
    return;
  }
  openMainView(route);
}

function routesEqual(a: MainViewRoute, b: MainViewRoute): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "board" && b.kind === "board") return true;
  if (a.kind === "sessionDetail" && b.kind === "sessionDetail") {
    return a.sessionId === b.sessionId;
  }
  if (a.kind === "externalTool" && b.kind === "externalTool") {
    return a.runId === b.runId;
  }
  if (a.kind === "preferences" && b.kind === "preferences") {
    return (
      (a.category ?? null) === (b.category ?? null) &&
      (a.externalToolId ?? null) === (b.externalToolId ?? null)
    );
  }
  return false;
}
