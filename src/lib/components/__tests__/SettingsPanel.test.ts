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
    cmdAgentNotificationSetupStatus: vi.fn().mockResolvedValue({
      providers: [
        {
          provider: "claude",
          label: "Claude Code",
          status: "installed",
          detail: "Claude Code hooks are installed.",
          configPath: null,
          installable: true,
        },
        {
          provider: "codex",
          label: "Codex",
          status: "missing",
          detail: "notification_condition is not set.",
          configPath: "/tmp/codex/config.toml",
          installable: true,
        },
      ],
    }),
    cmdPreviewCodexNotificationConfig: vi.fn().mockResolvedValue({
      status: "ok",
      data: {
        configPath: "/tmp/codex/config.toml",
        configured: false,
        currentValue: null,
        nextContent: '[tui]\nnotification_condition = "always"\n',
      },
    }),
    cmdConfigureCodexNotificationConfig: vi.fn().mockResolvedValue({
      status: "ok",
      data: null,
    }),
    reinstallHooks: vi.fn().mockResolvedValue({
      status: "ok",
      data: null,
    }),
    cmdMcpStatus: vi.fn().mockResolvedValue({
      enabled: true,
      cliInstalled: true,
      cliCurrent: true,
      cliPath: "/tmp/roux",
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
        nextEntryJson: '{\n  "command": "/tmp/roux",\n  "args": [\n    "mcp"\n  ]\n}',
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
  checkDoctorStatus: vi.fn().mockResolvedValue({
    items: [],
  }),
  installAllMissing: vi.fn().mockResolvedValue(undefined),
  reinstallCli: vi.fn().mockResolvedValue(undefined),
  reinstallHooks: vi.fn().mockResolvedValue(undefined),
  reinstallSkill: vi.fn().mockResolvedValue(undefined),
  getRuntimeStatus: vi.fn().mockResolvedValue({
    mode: "daemon",
    startedAtMs: 1_700_000_000_000,
    uptimeMs: 12_345,
    daemon: {
      kind: "roux-daemon",
      pid: 4242,
      socket: "/tmp/roux.sock",
      logPath: "/tmp/roux-daemon.log",
      startedAtMs: 1_700_000_000_000,
      uptimeMs: 12_345,
      sessionCount: 2,
      projectCount: 3,
      processCount: 4,
      ptyCount: 5,
      capabilities: ["daemon-status", "daemon-pty-list"],
    },
  }),
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
import { getRuntimeStatus, updateSettings } from "$lib/tauri";

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

describe("SettingsPanel agent notification setup", () => {
  beforeEach(() => {
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(commands.cmdAgentNotificationSetupStatus).mockClear();
    vi.mocked(commands.cmdPreviewCodexNotificationConfig).mockClear();
    vi.mocked(commands.cmdConfigureCodexNotificationConfig).mockClear();
  });

  it("renders agent provider status and configures Codex notifications", async () => {
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Notifications" }));

    expect(await screen.findByText("Agent notifications")).toBeDefined();
    expect(await screen.findByText("notification_condition is not set.")).toBeDefined();
    expect(await screen.findByText("/tmp/codex/config.toml")).toBeDefined();

    await fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    await waitFor(() => {
      expect(commands.cmdPreviewCodexNotificationConfig).toHaveBeenCalled();
    });
    expect(await screen.findByText("Codex config preview ready.")).toBeDefined();
    expect(await screen.findByText(/notification_condition = "always"/)).toBeDefined();

    await fireEvent.click(screen.getByRole("button", { name: "Configure" }));

    await waitFor(() => {
      expect(commands.cmdConfigureCodexNotificationConfig).toHaveBeenCalled();
    });
    expect(await screen.findByText("Codex notifications configured.")).toBeDefined();
  });

  it("keeps provider actions disabled while agent notification status is loading", async () => {
    vi.mocked(commands.cmdAgentNotificationSetupStatus).mockReturnValue(new Promise(() => {}));
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Notifications" }));

    expect(await screen.findByText("Agent notifications")).toBeDefined();
    expect((screen.getByRole("button", { name: "Preview" }) as HTMLButtonElement).disabled)
      .toBe(true);
    for (const button of screen.getAllByRole("button", { name: "Configure" })) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }
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

    const select = (await screen.findByRole("combobox", {
      name: "Select Example variant",
    })) as HTMLSelectElement;
    expect(select.value).toBe("a");
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
    const select = (await screen.findByRole("combobox", {
      name: "Select Example variant",
    })) as HTMLSelectElement;

    await fireEvent.change(select, { target: { value: "c" } });

    await waitFor(() => {
      expect(updateSettings).toHaveBeenCalled();
    });
    const lastCall = vi.mocked(updateSettings).mock.calls.at(-1)!;
    expect(lastCall[0].experiments?.exampleVariant).toBe("c");
    expect(lastCall[0].experiments?.exampleFlag).toBe(false);
  });
});

describe("SettingsPanel runtime debug", () => {
  beforeEach(() => {
    settings.set({ ...DEFAULT_SETTINGS });
    vi.mocked(getRuntimeStatus).mockClear();
  });

  it("renders daemon runtime status on the Advanced page", async () => {
    render(SettingsPanel, { visible: true, onclose: vi.fn() });

    await fireEvent.click(screen.getByRole("button", { name: "Advanced" }));

    expect(await screen.findByText("Runtime")).toBeDefined();
    expect((await screen.findAllByText("Daemon")).length).toBeGreaterThan(0);
    expect(await screen.findByText("pid 4242")).toBeDefined();
    expect(await screen.findByText("/tmp/roux.sock")).toBeDefined();
    expect(getRuntimeStatus).toHaveBeenCalled();
  });
});
