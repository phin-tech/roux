import { registry } from "./registry";
import { queries } from "$lib/queries";
import { createWatch } from "$lib/tauri";
import type { CreateWatchConfig, WatchKind } from "$lib/types";

export function registerWatchCommands() {
  registry.register({
    id: "watch.add",
    label: "Add Watch",
    category: "Watches",
    getItems: () => [
      {
        id: "github-action",
        label: "GitHub Action",
        description: "Watch a GitHub Actions workflow run",
        drillCommand: "watch.add-github",
      },
      {
        id: "http-health",
        label: "HTTP Health Check",
        description: "Monitor a URL for availability",
        drillCommand: "watch.add-http",
      },
      {
        id: "shell-command",
        label: "Shell Command",
        description: "Run a command and watch exit code",
        drillCommand: "watch.add-shell",
      },
      {
        id: "github-pr",
        label: "GitHub Pull Request",
        description: "Watch a PR for reviews, checks, and state",
        drillCommand: "watch.add-github-pr",
      },
    ],
  });

  registry.register({
    id: "watch.add-http",
    label: "Add HTTP Watch",
    category: "Watches",
    inputPlaceholder: "Enter URL to monitor (e.g. https://api.example.com/health)...",
    getItems: () => [],
    onInput: async (url: string) => {
      if (!url.startsWith("http")) return;
      let parsedUrl: URL;
      try {
        parsedUrl = new URL(url);
      } catch {
        return;
      }
      const session = queries.activeSession();
      const config: CreateWatchConfig = {
        name: `Health: ${parsedUrl.hostname}`,
        kind: { type: "httpHealth", url, expectedStatus: 200 },
        mode: { type: "recurring", intervalSecs: 60 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
        notify: null,
      };
      await createWatch(config);
    },
  });

  registry.register({
    id: "watch.add-shell",
    label: "Add Shell Command Watch",
    category: "Watches",
    inputPlaceholder: "Enter command to watch (e.g. curl -s http://localhost:3000)...",
    getItems: () => [],
    onInput: async (command: string) => {
      if (!command.trim()) return;
      const session = queries.activeSession();
      const config: CreateWatchConfig = {
        name: `Cmd: ${command.slice(0, 40)}`,
        kind: {
          type: "shellCommand",
          command,
          workingDir: session?.worktreePath ?? null,
          successExitCode: 0,
        },
        mode: { type: "recurring", intervalSecs: 30 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
        notify: null,
      };
      await createWatch(config);
    },
  });

  registry.register({
    id: "watch.add-github",
    label: "Add GitHub Action Watch",
    category: "Watches",
    inputPlaceholder: "Enter repo (owner/name) or GitHub Actions URL...",
    getItems: () => [],
    onInput: async (input: string) => {
      if (!input.trim()) return;
      const session = queries.activeSession();
      const urlMatch = input.match(
        /github\.com\/([^/]+\/[^/]+)\/actions\/runs\/(\d+)/
      );
      let kind: WatchKind;
      let name: string;
      if (urlMatch) {
        kind = {
          type: "githubAction",
          repo: urlMatch[1],
          runId: parseInt(urlMatch[2], 10),
          workflow: null,
          branch: null,
        };
        name = `GH: ${urlMatch[1]} #${urlMatch[2]}`;
      } else {
        kind = {
          type: "githubAction",
          repo: input.trim(),
          runId: null,
          workflow: null,
          branch: null,
        };
        name = `GH: ${input.trim()}`;
      }
      const config: CreateWatchConfig = {
        name,
        kind,
        mode: urlMatch ? { type: "oneShot" } : { type: "recurring", intervalSecs: 30 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
        notify: null,
      };
      await createWatch(config);
    },
  });

  registry.register({
    id: "watch.add-github-pr",
    label: "Add GitHub PR Watch",
    category: "Watches",
    inputPlaceholder: "Enter PR URL (e.g. https://github.com/owner/repo/pull/123) or owner/repo#123...",
    getItems: () => [],
    onInput: async (input: string) => {
      if (!input.trim()) return;
      const session = queries.activeSession();
      const urlMatch = input.match(/github\.com\/([^/]+\/[^/]+)\/pull\/(\d+)/);
      const shortMatch = input.match(/^([^#]+)#(\d+)$/);
      let repo: string;
      let prNumber: number;
      if (urlMatch) {
        repo = urlMatch[1];
        prNumber = parseInt(urlMatch[2], 10);
      } else if (shortMatch) {
        repo = shortMatch[1].trim();
        prNumber = parseInt(shortMatch[2], 10);
      } else {
        return;
      }
      const config: CreateWatchConfig = {
        name: `PR: ${repo} #${prNumber}`,
        kind: { type: "githubPr", repo, prNumber },
        mode: { type: "recurring", intervalSecs: 30 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
        notify: null,
      };
      await createWatch(config);
    },
  });
}
