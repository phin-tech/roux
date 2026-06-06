import { describe, expect, it } from "vitest";
import { buildWorkItemHistoryRows } from "../history";

describe("work item history core", () => {
  it("orders card events, runs, and attachments newest first", () => {
    const rows = buildWorkItemHistoryRows({
      cardEvents: [
        { kind: "created", createdAt: 1 },
        { kind: "archived", createdAt: 5 },
      ],
      runs: [
        {
          id: "run-1",
          kind: "implementation",
          status: "done",
          updatedAt: 4,
        },
      ],
      attachments: [{ id: "att-1", title: "Review feedback", updatedAt: 3 }],
    });

    expect(rows.map((row) => row.label)).toEqual([
      "Archived card",
      "Implementation run done",
      "Review feedback attached",
      "Created card",
    ]);
  });
});
