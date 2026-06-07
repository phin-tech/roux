import { describe, expect, it } from "vitest";
import defaultWorkflow from "../defaultWorkflow.json";
import {
  DEFAULT_KANBAN_SETTINGS,
  DEFAULT_WORKFLOW_SETTINGS,
} from "../workflow";

describe("workflow defaults", () => {
  it("uses the bundled JSON workflow as the default workflow", () => {
    expect(DEFAULT_WORKFLOW_SETTINGS).toEqual(defaultWorkflow);
    expect(DEFAULT_KANBAN_SETTINGS.workflow).toEqual(defaultWorkflow);
  });

  it("groups review gates inside the review phase", () => {
    expect(Object.keys(DEFAULT_WORKFLOW_SETTINGS.phases)).toEqual([
      "planning",
      "implementation",
      "review",
    ]);
    expect(Object.keys(DEFAULT_WORKFLOW_SETTINGS.phases.review.stages)).toEqual(
      ["local_review", "pr_review"],
    );
  });
});
