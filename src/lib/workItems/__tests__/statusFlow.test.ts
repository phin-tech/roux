import { describe, expect, it } from "vitest";
import { nextWorkItemStatuses } from "../statusFlow";

describe("work item status flow", () => {
  it("exposes only the next workflow status", () => {
    expect(nextWorkItemStatuses("todo")).toEqual(["planning"]);
    expect(nextWorkItemStatuses("planning")).toEqual(["doing"]);
    expect(nextWorkItemStatuses("doing")).toEqual(["review"]);
    expect(nextWorkItemStatuses("review")).toEqual(["done"]);
    expect(nextWorkItemStatuses("done")).toEqual([]);
  });

  it("hides direct Review to Done moves when review acceptance owns completion", () => {
    expect(nextWorkItemStatuses("review", { reviewAcceptsDone: true })).toEqual(
      [],
    );
  });
});
