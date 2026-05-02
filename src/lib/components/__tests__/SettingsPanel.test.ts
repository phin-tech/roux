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
