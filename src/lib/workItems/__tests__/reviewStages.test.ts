import { describe, expect, it } from "vitest";
import { reviewStageLabel } from "../reviewStages";
import { reviewAgentProfileId } from "../workflow";

describe("reviewStageLabel", () => {
  it("resolves stable review stage ids to human labels", () => {
    expect(reviewStageLabel("local_review")).toBe("Local Review");
    expect(reviewStageLabel("pr_review")).toBe("PR Review");
  });

  it("uses workflow stage labels when configured", () => {
    expect(
      reviewStageLabel("local_review", {
        workflow: {
          phases: {
            review: {
              stages: {
                local_review: {
                  label: "Local QA",
                },
              },
            },
          },
        },
      }),
    ).toBe("Local QA");
  });

  it("preserves unknown ids and omits empty stages", () => {
    expect(reviewStageLabel("security_review")).toBe("security_review");
    expect(reviewStageLabel(null)).toBeNull();
    expect(reviewStageLabel(undefined)).toBeNull();
  });
});

describe("reviewAgentProfileId", () => {
  it("uses the stage profile before the review phase profile", () => {
    expect(
      reviewAgentProfileId(
        {
          workflow: {
            phases: {
              review: {
                agentProfile: "phase-review",
                stages: {
                  pr_review: {
                    agentProfile: "stage-review",
                  },
                },
              },
            },
          },
        },
        "pr_review",
      ),
    ).toBe("stage-review");
  });

  it("falls back to the review phase profile", () => {
    expect(
      reviewAgentProfileId(
        {
          workflow: {
            phases: {
              review: {
                agentProfile: "phase-review",
              },
            },
          },
        },
        "pr_review",
      ),
    ).toBe("phase-review");
  });
});
