import { describe, expect, it } from "vitest";

import { groupIntoThreads } from "$lib/stores/mailbox";
import type { MailboxEventPayload } from "$lib/tauri";

function event(
  id: string,
  createdAt: number,
  partial: Partial<MailboxEventPayload> = {},
): MailboxEventPayload {
  return {
    id,
    createdAt,
    to: "reviewer",
    topic: null,
    from: "me",
    kind: "task",
    correlationId: null,
    projectId: null,
    subject: null,
    body: `event ${id}`,
    structured: null,
    retractedAt: null,
    ...partial,
  };
}

describe("groupIntoThreads", () => {
  it("returns an empty array for an empty input", () => {
    expect(groupIntoThreads([])).toEqual([]);
  });

  it("treats events without correlationId as singleton threads", () => {
    const a = event("a", 1);
    const b = event("b", 2);
    const threads = groupIntoThreads([a, b]);
    expect(threads).toHaveLength(2);
    expect(threads[0].root.id).toBe("a");
    expect(threads[0].replies).toEqual([]);
    expect(threads[1].root.id).toBe("b");
  });

  it("groups correlated events under the root whose id matches correlationId", () => {
    const root = event("root-1", 100);
    const reply1 = event("r1", 110, { correlationId: "root-1" });
    const reply2 = event("r2", 120, { correlationId: "root-1" });
    const threads = groupIntoThreads([reply2, root, reply1]);
    expect(threads).toHaveLength(1);
    expect(threads[0].id).toBe("root-1");
    expect(threads[0].root.id).toBe("root-1");
    // Replies oldest → newest.
    expect(threads[0].replies.map((e) => e.id)).toEqual(["r1", "r2"]);
  });

  it("falls back to earliest event when the correlationId root isn't in the slice", () => {
    // Original was clipped/dismissed; we only see the replies.
    const reply1 = event("r1", 110, { correlationId: "missing-root" });
    const reply2 = event("r2", 120, { correlationId: "missing-root" });
    const threads = groupIntoThreads([reply2, reply1]);
    expect(threads).toHaveLength(1);
    expect(threads[0].id).toBe("missing-root");
    expect(threads[0].root.id).toBe("r1"); // earliest visible
    expect(threads[0].replies.map((e) => e.id)).toEqual(["r2"]);
  });

  it("orders threads by root createdAt ascending (oldest thread first)", () => {
    const oldRoot = event("old", 100);
    const newRoot = event("new", 500);
    const newReply = event("new-r", 510, { correlationId: "new" });
    const threads = groupIntoThreads([newRoot, newReply, oldRoot]);
    expect(threads.map((t) => t.id)).toEqual(["old", "new"]);
  });

  it("breaks ordering ties on root createdAt with thread id (deterministic)", () => {
    const a = event("a", 100);
    const b = event("b", 100);
    const threads = groupIntoThreads([b, a]);
    expect(threads.map((t) => t.id)).toEqual(["a", "b"]);
  });

  it("keeps singletons and threads interleaved by createdAt", () => {
    const single1 = event("s1", 50);
    const root = event("root", 100);
    const reply = event("r", 200, { correlationId: "root" });
    const single2 = event("s2", 300);
    const threads = groupIntoThreads([single2, single1, reply, root]);
    expect(threads.map((t) => t.id)).toEqual(["s1", "root", "s2"]);
    const rootThread = threads.find((t) => t.id === "root")!;
    expect(rootThread.replies).toHaveLength(1);
    expect(rootThread.replies[0].id).toBe("r");
  });

  it("self-correlated event (correlationId === id) is a 1-event thread", () => {
    // Defensive: backend's `mailbox reply` seeds correlationId to the
    // original event id, but a malformed sender could set it to its
    // own id. Should not duplicate the root into replies.
    const e = event("loop", 100, { correlationId: "loop" });
    const threads = groupIntoThreads([e]);
    expect(threads).toHaveLength(1);
    expect(threads[0].root.id).toBe("loop");
    expect(threads[0].replies).toEqual([]);
  });
});
