import { render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PtyPanel from "../PtyPanel.svelte";
import { listAllPtys } from "$lib/tauri";
import type { PtyInfo } from "$lib/types";

vi.mock("$lib/tauri", () => ({
  listAllPtys: vi.fn(),
}));

function pty(overrides: Partial<PtyInfo> = {}): PtyInfo {
  return {
    id: "pty-1",
    session_id: "session-1",
    role: "sessionPrimary",
    status: { type: "RunningDetached", since_ms: 1 },
    name: "Planner",
    working_dir: "/repo",
    profile: "claude",
    unread_output: false,
    bell_pending: false,
    ...overrides,
  };
}

describe("PtyPanel", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("lists daemon-managed PTYs when visible", async () => {
    vi.mocked(listAllPtys).mockResolvedValue([
      pty(),
      pty({
        id: "pty-2",
        name: "Shell",
        role: "secondary",
        status: { type: "RunningAttached", pane_id: "pane-2" },
      }),
    ]);

    render(PtyPanel, {
      visible: true,
      onclose: vi.fn(),
    });

    await waitFor(() => expect(listAllPtys).toHaveBeenCalled());
    expect(await screen.findByText("Planner")).toBeDefined();
    expect(screen.getByText("Shell")).toBeDefined();
    expect(screen.getByText("status: detached")).toBeDefined();
    expect(screen.getByText("status: attached to pane-2")).toBeDefined();
  });

  it("refreshes while visible so newly spawned planning PTYs appear", async () => {
    vi.mocked(listAllPtys)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        pty({
          id: "planning-pty",
          name: "Planning run",
          status: { type: "RunningAttached", pane_id: "session-1-main" },
        }),
      ]);

    render(PtyPanel, {
      visible: true,
      onclose: vi.fn(),
    });

    await waitFor(() => expect(listAllPtys).toHaveBeenCalledTimes(1));
    expect(screen.getByText("No daemon PTYs")).toBeDefined();

    await vi.advanceTimersByTimeAsync(2_000);

    await waitFor(() => expect(listAllPtys).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Planning run")).toBeDefined();
  });
});
