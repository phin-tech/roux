import { describe, expect, it } from "vitest";
import {
  canStartImplementationFromPlanning,
  hasAttachedPlan,
  isPlanAttachment,
} from "../planningGate";
import type { Attachment } from "$lib/types/workItems";

function attachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: "att-1",
    documentId: "wi-1.att-1",
    targetKind: "workItem",
    targetId: "wi-1",
    title: null,
    contentKind: "text",
    mimeType: null,
    sourcePath: null,
    byteLen: 12,
    sha256: "sha",
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("planning gate", () => {
  it("detects an attached plan from its title", () => {
    expect(isPlanAttachment(attachment({ title: "Plan" }))).toBe(true);
    expect(isPlanAttachment(attachment({ title: "Implementation Plan" }))).toBe(
      true,
    );
  });

  it("detects an attached plan from a plan-like markdown source path", () => {
    expect(
      isPlanAttachment(
        attachment({
          sourcePath: "/tmp/kanban-workflow-plan.md",
          mimeType: "text/markdown",
        }),
      ),
    ).toBe(true);
  });

  it("matches daemon plan tokens across common file names", () => {
    expect(
      isPlanAttachment(
        attachment({
          sourcePath: "/tmp/implementation_plan.md",
          mimeType: "text/markdown",
        }),
      ),
    ).toBe(true);
    expect(
      isPlanAttachment(
        attachment({
          sourcePath: "/tmp/plan.txt",
          mimeType: "text/plain",
        }),
      ),
    ).toBe(true);
  });

  it("does not treat unrelated markdown attachments as plans", () => {
    expect(
      isPlanAttachment(
        attachment({
          title: "Research Notes",
          sourcePath: "/tmp/notes.md",
          mimeType: "text/markdown",
        }),
      ),
    ).toBe(false);
  });

  it("requires a plan attachment unless the user forces implementation", () => {
    expect(canStartImplementationFromPlanning([], false)).toBe(false);
    expect(canStartImplementationFromPlanning([], true)).toBe(true);
    expect(
      canStartImplementationFromPlanning(
        [attachment({ title: "Plan" })],
        false,
      ),
    ).toBe(true);
  });

  it("groups plan detection over the full attachment list", () => {
    expect(
      hasAttachedPlan([
        attachment({ id: "att-1", title: "Notes" }),
        attachment({ id: "att-2", title: "Plan" }),
      ]),
    ).toBe(true);
  });
});
