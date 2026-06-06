import { describe, expect, it } from "vitest";
import { reviewStageLabel } from "../reviewStages";

describe("reviewStageLabel", () => {
  it("resolves stable review stage ids to human labels", () => {
    expect(reviewStageLabel("local_review")).toBe("Local Review");
    expect(reviewStageLabel("pr_review")).toBe("PR Review");
  });

  it("preserves unknown ids and omits empty stages", () => {
    expect(reviewStageLabel("security_review")).toBe("security_review");
    expect(reviewStageLabel(null)).toBeNull();
    expect(reviewStageLabel(undefined)).toBeNull();
  });
});
