import { describe, expect, it, beforeEach, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  onBusSubscriptionEvent: vi.fn().mockResolvedValue(() => {}),
  subscriptionsCreate: vi.fn(),
  subscriptionsDelete: vi.fn(),
  subscriptionsList: vi.fn().mockResolvedValue([]),
}));

import {
  applySubscriptionEvent,
  subscriptions,
} from "$lib/stores/subscriptions";
import type { BusSubscription } from "$lib/tauri";

function fixture(id: string, alias = "auditor", pattern = "*"): BusSubscription {
  return {
    id,
    alias,
    pattern,
    projectId: null,
    createdAt: 1000,
  };
}

describe("subscriptions store", () => {
  beforeEach(() => {
    subscriptions.set([]);
  });

  it("created event prepends a new subscription", () => {
    applySubscriptionEvent({ kind: "created", subscription: fixture("a") });
    applySubscriptionEvent({ kind: "created", subscription: fixture("b") });
    const list = get(subscriptions);
    expect(list.map((s) => s.id)).toEqual(["b", "a"]);
  });

  it("created event for an existing id replaces in place (idempotent reload)", () => {
    applySubscriptionEvent({ kind: "created", subscription: fixture("a", "x") });
    applySubscriptionEvent({
      kind: "created",
      subscription: { ...fixture("a", "y"), pattern: "**" },
    });
    const list = get(subscriptions);
    expect(list).toHaveLength(1);
    expect(list[0].alias).toBe("y");
    expect(list[0].pattern).toBe("**");
  });

  it("removed event drops the matching id", () => {
    applySubscriptionEvent({ kind: "created", subscription: fixture("a") });
    applySubscriptionEvent({ kind: "created", subscription: fixture("b") });
    applySubscriptionEvent({ kind: "removed", id: "a" });
    expect(get(subscriptions).map((s) => s.id)).toEqual(["b"]);
  });

  it("removed event for unknown id is a no-op", () => {
    applySubscriptionEvent({ kind: "created", subscription: fixture("a") });
    applySubscriptionEvent({ kind: "removed", id: "ghost" });
    expect(get(subscriptions).map((s) => s.id)).toEqual(["a"]);
  });
});
