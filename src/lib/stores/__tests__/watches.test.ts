import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  watchState,
  watchesForSession,
  failureCount,
  addOrUpdateWatch,
  removeWatchFromStore,
} from "../watches";
import type { Watch } from "$lib/types";

function makeWatch(overrides: Partial<Watch> = {}): Watch {
  return {
    id: crypto.randomUUID(),
    name: "test-watch",
    kind: { type: "httpHealth", url: "http://localhost", expectedStatus: 200 },
    mode: { type: "recurring", intervalSecs: 60 },
    scope: { type: "global" },
    runtimeState: { type: "active" },
    lastResult: null,
    lastChecked: null,
    notify: { desktopNotification: true, onFailure: true, onSuccess: false },
    createdAt: Date.now(),
    ...overrides,
  };
}

describe("watches store", () => {
  beforeEach(() => {
    watchState.set([]);
  });

  it("adds a watch", () => {
    const w = makeWatch({ name: "health" });
    addOrUpdateWatch(w);
    expect(get(watchState)).toHaveLength(1);
    expect(get(watchState)[0].name).toBe("health");
  });

  it("updates an existing watch by id", () => {
    const w = makeWatch({ name: "v1" });
    addOrUpdateWatch(w);
    addOrUpdateWatch({ ...w, name: "v2" });
    expect(get(watchState)).toHaveLength(1);
    expect(get(watchState)[0].name).toBe("v2");
  });

  it("removes a watch", () => {
    const w = makeWatch();
    addOrUpdateWatch(w);
    removeWatchFromStore(w.id);
    expect(get(watchState)).toHaveLength(0);
  });

  it("filters watches by session", () => {
    const w1 = makeWatch({ scope: { type: "session", sessionId: "s1" } });
    const w2 = makeWatch({ scope: { type: "session", sessionId: "s2" } });
    const w3 = makeWatch({ scope: { type: "global" } });
    addOrUpdateWatch(w1);
    addOrUpdateWatch(w2);
    addOrUpdateWatch(w3);

    const filtered = get(watchesForSession("s1"));
    expect(filtered).toHaveLength(1);
    expect(filtered[0].id).toBe(w1.id);
  });

  it("counts failures", () => {
    const w1 = makeWatch({
      lastResult: { type: "httpCheck", statusCode: 500, responseTimeMs: 100, outcome: "failure" },
    });
    const w2 = makeWatch({
      lastResult: { type: "httpCheck", statusCode: 200, responseTimeMs: 50, outcome: "success" },
    });
    addOrUpdateWatch(w1);
    addOrUpdateWatch(w2);
    expect(get(failureCount)).toBe(1);
  });
});
