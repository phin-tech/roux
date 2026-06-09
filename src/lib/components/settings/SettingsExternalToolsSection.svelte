<script lang="ts">
  import type {
    ExternalTool,
    ExternalToolSurface,
    ExternalToolWebEmbedder,
  } from "$lib/bindings";
  import { activeSession } from "$lib/stores/sessions";
  import { settings, updateSettingsDraft } from "$lib/stores/settings";
  import {
    previewExternalToolConfig,
    type RenderedExternalTool,
  } from "$lib/tauri";

  interface Props {
    focusedToolId?: string | null;
    focusToken?: number;
    onfocusapplied?: () => void;
  }

  let {
    focusedToolId = null,
    focusToken = 0,
    onfocusapplied,
  }: Props = $props();

  let expandedExternalToolId = $state<string | null>(null);
  let externalToolPreviewById = $state<
    Record<
      string,
      {
        loading: boolean;
        rendered: RenderedExternalTool | null;
        error: string | null;
      }
    >
  >({});
  const externalToolRowKeys = new Map<string, string>();
  let nextExternalToolRowKey = 0;
  let appliedFocusToken = 0;

  $effect(() => {
    if (focusToken === 0 || focusToken === appliedFocusToken) return;
    appliedFocusToken = focusToken;
    expandedExternalToolId = focusedToolId ?? null;
    onfocusapplied?.();
  });

  function externalTools(): ExternalTool[] {
    return $settings.externalTools ?? [];
  }

  function externalToolRowKey(id: string): string {
    let key = externalToolRowKeys.get(id);
    if (!key) {
      key = `external-tool-row-${++nextExternalToolRowKey}`;
      externalToolRowKeys.set(id, key);
    }
    return key;
  }

  function retainExternalToolRowKey(previousId: string, nextId: string): void {
    if (previousId === nextId) return;
    const key = externalToolRowKeys.get(previousId);
    if (!key) return;
    externalToolRowKeys.delete(previousId);
    externalToolRowKeys.set(nextId, key);
  }

  function pruneExternalToolRowKeys(tools: ExternalTool[]): void {
    const ids = new Set(tools.map((tool) => tool.id));
    for (const id of externalToolRowKeys.keys()) {
      if (!ids.has(id)) externalToolRowKeys.delete(id);
    }
  }

  function isStartupEligibleExternalTool(tool: ExternalTool): boolean {
    return tool.enabled !== false && !(tool.requiresSession ?? false);
  }

  function nextStartupExternalToolId(
    tools: ExternalTool[],
    currentId: string | null,
    shouldFallback: boolean,
  ): string | null {
    if (
      currentId &&
      tools.some(
        (tool) => tool.id === currentId && isStartupEligibleExternalTool(tool),
      )
    ) {
      return currentId;
    }
    return shouldFallback
      ? (tools.find(isStartupEligibleExternalTool)?.id ?? null)
      : null;
  }

  function createExternalToolId(): string {
    const existing = new Set(externalTools().map((tool) => tool.id));
    let id = `tool-${crypto.randomUUID()}`;
    while (existing.has(id)) {
      id = `tool-${crypto.randomUUID()}`;
    }
    return id;
  }

  function updateExternalTools(
    tools: ExternalTool[],
    startupToolRename: { previousId: string; nextId: string } | null = null,
  ): void {
    pruneExternalToolRowKeys(tools);
    updateSettingsDraft((current) => {
      const renamedStartupToolId =
        startupToolRename &&
        current.startupExternalToolId === startupToolRename.previousId
          ? startupToolRename.nextId
          : (current.startupExternalToolId ?? null);
      const startupExternalToolId = nextStartupExternalToolId(
        tools,
        renamedStartupToolId,
        current.startupTarget === "externalTool",
      );
      const startupTarget =
        current.startupTarget === "externalTool" &&
        startupExternalToolId === null
          ? "restore"
          : current.startupTarget;
      const renamedReviewDiffToolId =
        startupToolRename &&
        current.reviewDiffToolId === startupToolRename.previousId
          ? startupToolRename.nextId
          : (current.reviewDiffToolId ?? null);
      const reviewDiffToolId =
        renamedReviewDiffToolId &&
        tools.some(
          (tool) => tool.id === renamedReviewDiffToolId && tool.enabled !== false,
        )
          ? renamedReviewDiffToolId
          : null;
      return {
        ...current,
        externalTools: tools,
        startupExternalToolId,
        startupTarget,
        reviewDiffToolId,
      };
    });
  }

  function updateExternalTool(id: string, patch: Partial<ExternalTool>): void {
    const tools = externalTools();
    let nextPatch = patch;
    if (patch.id !== undefined) {
      const normalizedId = patch.id.trim();
      if (
        !normalizedId ||
        tools.some((tool) => tool.id !== id && tool.id.trim() === normalizedId)
      ) {
        return;
      }
      nextPatch = { ...patch, id: normalizedId };
    }
    if (nextPatch.id !== undefined) {
      retainExternalToolRowKey(id, nextPatch.id);
      if (expandedExternalToolId === id) expandedExternalToolId = nextPatch.id;
    }
    updateExternalTools(
      tools.map((tool) => (tool.id === id ? { ...tool, ...nextPatch } : tool)),
      nextPatch.id !== undefined
        ? { previousId: id, nextId: nextPatch.id }
        : null,
    );
  }

  function preferredPortFromInput(value: string): number | null {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number.parseInt(trimmed, 10);
    if (Number.isNaN(parsed)) return null;
    return Math.min(65535, Math.max(1, parsed));
  }

  function addExternalTool(surface: ExternalToolSurface): void {
    const id = createExternalToolId();
    const tool: ExternalTool = {
      id,
      name: surface === "web" ? "New Web Tool" : "New Terminal Tool",
      enabled: true,
      surface,
      commandTemplate:
        surface === "web" ? "server --port {{ port }}" : "command",
      cwdTemplate: "{{ session.worktree_path }}",
      requiresSession: true,
      urlTemplate: surface === "web" ? "http://127.0.0.1:{{ port }}" : null,
      preferredPort: surface === "web" ? 4966 : null,
      webEmbedder: "webview",
      keepWebviewAlive: false,
    };
    updateExternalTools([...externalTools(), tool]);
    expandedExternalToolId = id;
  }

  function removeExternalTool(id: string): void {
    updateExternalTools(externalTools().filter((tool) => tool.id !== id));
    if (expandedExternalToolId === id) expandedExternalToolId = null;
  }

  async function previewTool(tool: ExternalTool): Promise<void> {
    externalToolPreviewById = {
      ...externalToolPreviewById,
      [tool.id]: { loading: true, rendered: null, error: null },
    };
    try {
      const rendered = await previewExternalToolConfig(
        tool,
        tool.requiresSession ? ($activeSession?.id ?? null) : null,
        tool.surface === "web" ? (tool.preferredPort ?? 4966) : null,
      );
      externalToolPreviewById = {
        ...externalToolPreviewById,
        [tool.id]: { loading: false, rendered, error: null },
      };
    } catch (err) {
      externalToolPreviewById = {
        ...externalToolPreviewById,
        [tool.id]: {
          loading: false,
          rendered: null,
          error: err instanceof Error ? err.message : String(err),
        },
      };
    }
  }
</script>

<div class="mt-3 rounded-xl border border-border-subtle bg-bg-surface/35 p-3">
  <div class="flex items-center justify-between gap-2">
    <div>
      <div class="text-[13px] font-semibold">External Tools</div>
      <div class="mt-0.5 text-[11px] text-text-muted">
        Launch terminal and local web tools into the main view.
      </div>
    </div>
    <div class="flex gap-1">
      <button
        type="button"
        class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
        onclick={() => addExternalTool("terminal")}>Add Terminal</button
      >
      <button
        type="button"
        class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover"
        onclick={() => addExternalTool("web")}>Add Web</button
      >
    </div>
  </div>

  <!-- Review diff viewer selector -->
  <div class="mt-3 rounded border border-border-subtle bg-bg-deep/60 p-2.5">
    <div class="flex items-center gap-3">
      <label class="text-[12px] font-medium text-text-primary shrink-0" for="review-diff-tool">
        Review diff viewer
      </label>
      <select
        id="review-diff-tool"
        class="ml-auto min-w-0 max-w-[200px] rounded border border-border-subtle bg-bg-deep px-2 py-1 text-[11px] text-text-primary focus:border-accent-dim focus:outline-none focus:ring-1 focus:ring-accent-dim/50"
        value={$settings.reviewDiffToolId ?? ""}
        onchange={(e) => {
          const val = (e.currentTarget as HTMLSelectElement).value;
          updateSettingsDraft((s) => ({
            ...s,
            reviewDiffToolId: val || null,
          }));
        }}
      >
        <option value="">None</option>
        {#each externalTools().filter((t) => t.enabled !== false) as tool}
          <option value={tool.id}>{tool.name}</option>
        {/each}
      </select>
    </div>
    <div class="mt-1.5 text-[10px] text-text-muted leading-4">
      Tool launched by "View diff" in the work-item review modal. The tool's command
      template may use <code class="font-mono">{"{{ review.base }}"}</code> and
      <code class="font-mono">{"{{ review.changed_files }}"}</code> in addition to
      all session variables.
    </div>
  </div>

  <div class="mt-3 flex flex-col gap-2">
    {#each externalTools() as tool (externalToolRowKey(tool.id))}
      {@const expanded = expandedExternalToolId === tool.id}
      {@const preview = externalToolPreviewById[tool.id]}
      <div class="rounded border border-border-subtle bg-bg-deep/60 p-2">
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="min-w-0 flex-1 truncate text-left text-[12px] font-medium text-text-primary"
            onclick={() => (expandedExternalToolId = expanded ? null : tool.id)}
          >
            {tool.name}
          </button>
          <span
            class="rounded bg-bg-active px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-text-muted"
          >
            {tool.surface ?? "terminal"}
          </span>
          <label class="flex items-center gap-1 text-[11px] text-text-muted">
            <input
              type="checkbox"
              class="h-3 w-3 accent-accent"
              checked={tool.enabled !== false}
              onchange={(e) =>
                updateExternalTool(tool.id, {
                  enabled: e.currentTarget.checked,
                })}
            />
            Enabled
          </label>
          <button
            type="button"
            class="rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:text-red"
            onclick={() => removeExternalTool(tool.id)}>Remove</button
          >
        </div>

        {#if expanded}
          <div class="mt-3 grid gap-2">
            <label class="grid gap-1 text-[11px] text-text-muted">
              <span>Name</span>
              <input
                class="rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-primary outline-none focus:border-accent-dim"
                value={tool.name}
                oninput={(e) =>
                  updateExternalTool(tool.id, { name: e.currentTarget.value })}
              />
            </label>
            <div class="grid gap-2 md:grid-cols-[1fr_1fr]">
              <label class="grid gap-1 text-[11px] text-text-muted">
                <span>ID</span>
                <input
                  class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                  value={tool.id}
                  oninput={(e) =>
                    updateExternalTool(tool.id, { id: e.currentTarget.value })}
                />
              </label>
              <label class="grid gap-1 text-[11px] text-text-muted">
                <span>Surface</span>
                <input
                  class="rounded border border-border bg-bg-deep px-2 py-1 text-xs text-text-secondary"
                  value={tool.surface ?? "terminal"}
                  readonly
                />
              </label>
            </div>
            <label class="grid gap-1 text-[11px] text-text-muted">
              <span
                >Command template{(tool.surface ?? "terminal") === "web"
                  ? " (optional)"
                  : ""}</span
              >
              <textarea
                class="min-h-16 rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                value={tool.commandTemplate}
                oninput={(e) =>
                  updateExternalTool(tool.id, {
                    commandTemplate: e.currentTarget.value,
                  })}
              ></textarea>
            </label>
            <label class="grid gap-1 text-[11px] text-text-muted">
              <span>CWD template</span>
              <input
                class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                value={tool.cwdTemplate ?? ""}
                oninput={(e) =>
                  updateExternalTool(tool.id, {
                    cwdTemplate: e.currentTarget.value,
                  })}
              />
            </label>
            {#if (tool.surface ?? "terminal") === "web"}
              <div class="grid gap-1 text-[11px] text-text-muted">
                <span>Embedder</span>
                <div
                  class="inline-flex w-fit overflow-hidden rounded border border-border bg-bg-deep"
                >
                  {#each [{ value: "iframe", label: "Iframe" }, { value: "webview", label: "Webview" }] as option}
                    <button
                      type="button"
                      class="px-2 py-1 text-[11px] transition-colors {tool.webEmbedder ===
                      option.value
                        ? 'bg-bg-active text-text-primary'
                        : 'text-text-muted hover:bg-bg-hover hover:text-text-secondary'}"
                      onclick={() =>
                        updateExternalTool(tool.id, {
                          webEmbedder: option.value as ExternalToolWebEmbedder,
                          keepWebviewAlive:
                            option.value === "webview"
                              ? (tool.keepWebviewAlive ?? false)
                              : false,
                        })}
                    >
                      {option.label}
                    </button>
                  {/each}
                </div>
              </div>
              {#if tool.webEmbedder === "webview"}
                <label
                  class="flex items-center gap-2 text-[11px] text-text-secondary"
                >
                  <input
                    type="checkbox"
                    class="h-3 w-3 accent-accent"
                    checked={tool.keepWebviewAlive === true}
                    onchange={(e) =>
                      updateExternalTool(tool.id, {
                        keepWebviewAlive: e.currentTarget.checked,
                      })}
                  />
                  Keep webview active
                </label>
              {/if}
              <div class="grid gap-2 md:grid-cols-[1fr_120px]">
                <label class="grid gap-1 text-[11px] text-text-muted">
                  <span>URL template</span>
                  <input
                    class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                    value={tool.urlTemplate ?? ""}
                    oninput={(e) =>
                      updateExternalTool(tool.id, {
                        urlTemplate: e.currentTarget.value || null,
                      })}
                  />
                </label>
                <label class="grid gap-1 text-[11px] text-text-muted">
                  <span>Preferred port</span>
                  <input
                    type="number"
                    min="1"
                    max="65535"
                    class="rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
                    value={tool.preferredPort ?? ""}
                    oninput={(e) =>
                      updateExternalTool(tool.id, {
                        preferredPort: preferredPortFromInput(
                          e.currentTarget.value,
                        ),
                      })}
                  />
                </label>
              </div>
            {/if}
            <label
              class="flex items-center gap-2 text-[11px] text-text-secondary"
            >
              <input
                type="checkbox"
                class="h-3 w-3 accent-accent"
                checked={tool.requiresSession ?? false}
                onchange={(e) =>
                  updateExternalTool(tool.id, {
                    requiresSession: e.currentTarget.checked,
                  })}
              />
              Requires active session
            </label>
            <div class="flex items-center gap-2">
              <button
                type="button"
                class="rounded border border-border bg-bg-elevated px-2 py-1 text-[11px] text-text-secondary hover:bg-bg-hover disabled:opacity-40"
                disabled={preview?.loading}
                onclick={() => void previewTool(tool)}
                >{preview?.loading ? "Previewing" : "Preview Render"}</button
              >
              {#if tool.requiresSession && !$activeSession}
                <span class="text-[11px] text-amber"
                  >Preview needs an active session.</span
                >
              {/if}
            </div>
            {#if preview?.error}
              <div
                class="rounded border border-red/25 bg-red/10 p-2 text-[11px] text-red"
              >
                {preview.error}
              </div>
            {:else if preview?.rendered}
              <div
                class="grid gap-1 rounded border border-border-subtle bg-bg-deep/70 p-2 font-mono text-[10px] text-text-secondary"
              >
                <div>
                  <span class="text-text-muted">cmd</span>
                  {preview.rendered.command}
                </div>
                <div>
                  <span class="text-text-muted">cwd</span>
                  {preview.rendered.cwd}
                </div>
                {#if preview.rendered.url}
                  <div>
                    <span class="text-text-muted">url</span>
                    {preview.rendered.url}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>
