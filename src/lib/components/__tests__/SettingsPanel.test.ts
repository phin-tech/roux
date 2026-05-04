import { beforeEach, describe, expect, it, vi } from "vitest";
import { writable } from "svelte/store";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { DEFAULT_SETTINGS } from "$lib/types";

vi.mock("$lib/bindings", () => ({
  commands: {
    cmdDetectGh: vi.fn().mockResolvedValue({ binaryPath: null, version: null }),
    cmdDetectGit: vi.fn().mockResolvedValue({ binaryPath: null, version: null }),
    cmdDetectWorktrunk: vi.fn().mockResolvedValue({
      binaryPath: null,
      version: null,
      hasConfig: false,
    }),
    cmdPreviewWorktreeBase: vi.fn().mockResolvedValue(""),
    cmdMcpStatus: vi.fn().mockResolvedValue({
      enabled: true,
      cliInstalled: true,
      cliCurrent: true,
      cliPath: "/tmp/roux-cli",
      lastConfiguredHost: null,
      lastConfiguredAtMs: null,
      hosts: [
        {
          id: "claudeDesktop",
          label: "Claude Desktop",
          configPath: "/tmp/claude_desktop_config.json",
          configExists: false,
          configured: false,
          error: null,
        },
      ],
    }),
    cmdPreviewMcpHostConfig: vi.fn().mockResolvedValue({
      status: "ok",
      data: {
        host: "claudeDesktop",
        label: "Claude Desktop",
        configPath: "/tmp/claude_desktop_config.json",
        configExists: false,
        action: "create",
        configured: false,
        currentEntryJson: null,
        nextEntryJson: '{\n  "command": "/tmp/roux-cli",\n  "args": [\n    "mcp"\n  ]\n}',
      },
    }),
    cmdConfigureMcpHost: vi.fn(),
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn().mockResolvedValue("0.0.0-test") }));
vi.mock("$lib/logging", () => ({
  getLogPath: vi.fn().mockReturnValue("/tmp/roux.log"),
  setLoggingEnabled: vi.fn(),
}));
vi.mock("$lib/tauri", () => ({
  notificationsPush: vi.fn(),
  quitApp: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn().mockResolvedValue(undefined),
  onSettingsChanged: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("$lib/stores/updater", () => ({
  updateStatus: writable({ kind: "idle" }),
  runManualCheck: vi.fn(),
  performInstall: vi.fn(),
}));

import { commands } from "$lib/bindings";
import SettingsPanel from "../SettingsPanel.svelte";
import { settings } from "$lib/stores/settings";
import { updateSettings } from "$lib/tauri";

describe("SettingsPanel MCP integration", () => {
  beforeEach(() => {
    settings.set({ ...DEFAULT_SETTINGS, mcpEnabled: true });
    vi.mocked(commands.cmdMcpStatus).mockClear();
    vi.mocked(commands.cmdPreviewMcpHostConfig).mockClear();
  });

  it("renders Agent Integrations MCP setup and previews host config", async () => {
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Integrations" }));

    expect(await screen.findByText("Roux MCP")).toBeDefined();
    expect(await screen.findByText("Claude Desktop")).toBeDefined();

    await fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    await waitFor(() => {
      expect(commands.cmdPreviewMcpHostConfig).toHaveBeenCalledWith("claudeDesktop");
    });
    expect(await screen.findByText("Preview ready.")).toBeDefined();
  });
});

describe("SettingsPanel Experiments tab", () => {
  beforeEach(() => {
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(updateSettings).mockClear();
  });

  it("renders boolean and enum experiments at their declared defaults", async () => {
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Experiments" }));

    const toggle = await screen.findByRole("button", { name: "Toggle Example flag" });
    expect(toggle).toBeDefined();

    expect(await screen.findByText("Example variant")).toBeDefined();
    const select = document.querySelector(
      "select",
    ) as HTMLSelectElement | null;
    expect(select).not.toBeNull();
    expect(select!.value).toBe("a");
  });

  it("toggling a boolean experiment persists the new value", async () => {
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Experiments" }));
    await fireEvent.click(
      await screen.findByRole("button", { name: "Toggle Example flag" }),
    );

    await waitFor(() => {
      expect(updateSettings).toHaveBeenCalled();
    });
    const lastCall = vi.mocked(updateSettings).mock.calls.at(-1)!;
    expect(lastCall[0].experiments?.exampleFlag).toBe(true);
    // Sibling enum value must be preserved through the spread.
    expect(lastCall[0].experiments?.exampleVariant).toBe("a");
  });

  it("changing an enum experiment persists the new variant", async () => {
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Experiments" }));
    const select = (await screen.findByText("Example variant"))
      .closest("div.flex")!
      .querySelector("select") as HTMLSelectElement;

    await fireEvent.change(select, { target: { value: "c" } });

    await waitFor(() => {
      expect(updateSettings).toHaveBeenCalled();
    });
    const lastCall = vi.mocked(updateSettings).mock.calls.at(-1)!;
    expect(lastCall[0].experiments?.exampleVariant).toBe("c");
    expect(lastCall[0].experiments?.exampleFlag).toBe(false);
  });
});
