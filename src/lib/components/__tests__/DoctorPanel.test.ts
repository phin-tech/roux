import { render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { checkDoctorStatus } = vi.hoisted(() => ({
  checkDoctorStatus: vi.fn(),
}));

vi.mock("$lib/tauri", () => ({
  checkDoctorStatus,
  installAllMissing: vi.fn().mockResolvedValue(undefined),
  reinstallCli: vi.fn().mockResolvedValue(undefined),
  reinstallHooks: vi.fn().mockResolvedValue(undefined),
  reinstallSkill: vi.fn().mockResolvedValue(undefined),
}));

import DoctorPanel from "../DoctorPanel.svelte";

describe("DoctorPanel", () => {
  beforeEach(() => {
    checkDoctorStatus.mockReset();
  });

  it("shows startup notices from the backend", async () => {
    checkDoctorStatus.mockResolvedValue({
      notices: [
        "Homebrew roux detected at /opt/homebrew/bin/roux. Run brew upgrade phin-tech/tap/roux-pre to update it.",
      ],
      items: [],
    });

    render(DoctorPanel, { mode: "onboarding", visible: true });

    expect(await screen.findByText(/Homebrew roux detected/)).toBeTruthy();
    expect(
      screen.getByText(/brew upgrade phin-tech\/tap\/roux-pre/),
    ).toBeTruthy();
  });
});
