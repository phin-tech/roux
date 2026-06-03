<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { open } from "@tauri-apps/plugin-dialog";
  import { effectiveDefaultAgentProfileId } from "$lib/panes/defaultAgent";
  import { profileList, type SpawnProfile } from "$lib/panes/profiles";
  import {
    createProjectFull,
    updateProject,
    removeProject,
  } from "$lib/stores/projects";
  import { spawnBlueprintForProject } from "$lib/sessions/spawnBlueprint";
  import type { Project, SessionBlueprint } from "$lib/types";
  import { logError } from "$lib/logging";
  import { settings } from "$lib/stores/settings";
  import { sessionState } from "$lib/stores/sessions";
  import { listGitReposInRoots } from "$lib/tauri";
  import RepoAutoComplete from "./RepoAutoComplete.svelte";
  import {
    buildQuickPickOptions,
    type RepoQuickPickOption,
  } from "$lib/repos/quickPick";
  import {
    buildProjectPromptPreviewContext,
    renderProjectPromptTemplate,
  } from "$lib/projectPromptTemplates";

  interface Props {
    visible: boolean;
    /** When set, the dialog opens in edit mode and saves via update_project. */
    project?: Project | null;
    onclose: () => void;
  }

  let { visible, project = null, onclose }: Props = $props();

  // Form state — initialized from `project` prop on open.
  let name = $state("");
  let repoRoots = $state<string[]>([]);
  let contextPaths = $state<string[]>([]);
  let blueprints = $state<SessionBlueprint[]>([]);
  let projectPrompt = $state("");
  let promptPreviewBlueprintId = $state("");
  let promptPreview = $state("");
  let promptPreviewError = $state("");
  let promptPreviewing = $state(false);
  let error = $state("");
  let saving = $state(false);
  let confirmDelete = $state(false);

  // Per-row create-mode flag. Creating a project always spawns the listed
  // rows as live sessions — that's the dominant use case. `keepAsTemplate`
  // is an opt-in for the (rare) "also persist this as a project blueprint
  // so I can re-spawn it later" case.
  let keepAsTemplate = $state(new Set<string>());

  function isKeptAsTemplate(id: string): boolean {
    return keepAsTemplate.has(id);
  }

  function setKeepAsTemplate(id: string, keep: boolean) {
    const next = new Set(keepAsTemplate);
    if (keep) next.add(id);
    else next.delete(id);
    keepAsTemplate = next;
  }

  function dropTemplateFlag(id: string) {
    if (!keepAsTemplate.has(id)) return;
    const next = new Set(keepAsTemplate);
    next.delete(id);
    keepAsTemplate = next;
  }

  // Draft text bound to the repo autocomplete + the context-paths input.
  // The repo draft holds whatever the user is currently typing; on select
  // (or Enter with a non-matching path) it's pushed into `repoRoots` and
  // cleared, mirroring the multi-add flow this dialog needs.
  let repoDraft = $state("");
  let newPathDraft = $state("");

  // "Default session config" — the cross-repo-work primary use case. Click
  // Generate to seed one blueprint per repo using these defaults; per-row
  // edits afterward stay intact (we never overwrite an existing blueprint
  // when re-generating).
  let defaultBranch = $state("");
  let defaultBase = $state("");
  let defaultFetchFirst = $state(false);
  let defaultProfileChoice = $state("");
  let nameTemplate = $state("{{repo}}");
  // Svelte parses bare `{{...}}` in markup as expression shorthand, so the
  // template-token placeholder has to come through a constant string.
  const namePlaceholder = "{{repo}}-{{branch}}";

  // Discovered git repos under the user's configured `settings.repoRoots`,
  // mirrored from the same scan NewSessionDialog uses for its quick-pick.
  // Refreshed every time the dialog opens.
  let discoveredRepos = $state<string[]>([]);
  let discoveryLoading = $state(false);

  $effect(() => {
    if (!visible) return;
    name = project?.name ?? "";
    repoRoots = [...(project?.repoRoots ?? [])];
    contextPaths = [...(project?.contextPaths ?? [])];
    blueprints = (project?.sessionBlueprints ?? []).map((bp) => ({ ...bp }));
    projectPrompt = project?.projectPrompt ?? "";
    promptPreviewBlueprintId = "";
    promptPreview = "";
    promptPreviewError = "";
    promptPreviewing = false;
    error = "";
    saving = false;
    repoDraft = "";
    newPathDraft = "";
    defaultBranch = "";
    defaultBase = "";
    defaultFetchFirst = false;
    defaultProfileChoice = "";
    nameTemplate = "{{repo}}";
    confirmDelete = false;
    // Reset the per-row template flag. In edit mode this is unused (the
    // checkbox UI is hidden), so we just clear it.
    keepAsTemplate = new Set();
    void loadDiscoveredRepos();
  });

  async function loadDiscoveredRepos() {
    const roots = ($settings.repoRoots ?? [])
      .map((r) => r.trim())
      .filter(Boolean);
    if (roots.length === 0) {
      discoveredRepos = [];
      return;
    }
    discoveryLoading = true;
    try {
      discoveredRepos = await listGitReposInRoots(
        roots,
        $settings.excludeWorktreesFromRepoRoots ?? true,
      );
    } catch (e) {
      logError(`new-project: discover repos failed — ${e}`);
      discoveredRepos = [];
    } finally {
      discoveryLoading = false;
    }
  }

  // Quick-pick options shown in the autocomplete: only repos discovered
  // under settings roots that aren't already attached to this project.
  let repoOptions = $derived<RepoQuickPickOption[]>(
    buildQuickPickOptions(
      discoveredRepos.filter((r) => !repoRoots.includes(r)),
    ),
  );
  let hasConfiguredRoots = $derived(($settings.repoRoots ?? []).length > 0);

  function pushRepoRoot(path: string) {
    const trimmed = path.trim();
    if (!trimmed) return;
    if (!repoRoots.includes(trimmed)) repoRoots = [...repoRoots, trimmed];
    repoDraft = "";
  }

  const isEdit = $derived(project !== null);
  const profiles: SpawnProfile[] = $derived($profileList);
  const defaultProfileId = $derived(
    profiles.some(
      (profile) => profile.id === effectiveDefaultAgentProfileId($settings),
    )
      ? effectiveDefaultAgentProfileId($settings)
      : (profiles[0]?.id ?? "claude"),
  );
  const effectiveDefaultProfile = $derived(
    defaultProfileChoice || defaultProfileId,
  );
  const selectedPreviewBlueprint = $derived(
    blueprints.find((bp) => bp.id === promptPreviewBlueprintId) ??
      blueprints[0] ??
      null,
  );

  $effect(() => {
    const hasSelection = blueprints.some(
      (bp) => bp.id === promptPreviewBlueprintId,
    );
    if (!hasSelection) promptPreviewBlueprintId = blueprints[0]?.id ?? "";
  });

  $effect(() => {
    projectPrompt;
    promptPreviewBlueprintId;
    name;
    repoRoots;
    contextPaths;
    blueprints;
    promptPreview = "";
    promptPreviewError = "";
  });

  // Last segment of a repo path → human-friendly handle for the name
  // template. Mirrors how SessionTabs and the existing repo-grouping logic
  // identify a repo by its trailing directory name.
  function repoHandle(repoPath: string): string {
    const segments = repoPath.replaceAll("\\", "/").split("/").filter(Boolean);
    return segments[segments.length - 1] ?? repoPath;
  }

  /**
   * Apply the name template to a single repo. Empty placeholders collapse:
   * `{{repo}}-{{branch}}` with no branch becomes `repo` (not `repo-`). After
   * substitution we trim any stray separators left around removed tokens so
   * the generated names stay tidy regardless of which fields are filled.
   */
  function renderTemplate(template: string, repoPath: string): string {
    const out = template
      .replaceAll("{{repo}}", repoHandle(repoPath))
      .replaceAll("{{branch}}", defaultBranch.trim())
      .replaceAll("{{project}}", name.trim());
    // Collapse separator runs left by empty substitutions, then trim.
    return out
      .replace(/[-_/]{2,}/g, (m) => m[0])
      .replace(/^[-_/]+|[-_/]+$/g, "");
  }

  // Repos that don't yet have a blueprint pointing at them — these are the
  // candidates Generate will seed. Hidden once every repo has at least one.
  let reposWithoutBlueprint = $derived(
    repoRoots.filter((r) => !blueprints.some((bp) => bp.repoRoot === r)),
  );

  // Live preview of the names Generate will produce, so the user can see
  // their template applied before clicking the button.
  let previewNames = $derived(
    reposWithoutBlueprint.map((r) => renderTemplate(nameTemplate, r)),
  );

  function generateFromRepos() {
    if (reposWithoutBlueprint.length === 0) {
      error =
        "Every repo already has a session — remove one first to regenerate.";
      return;
    }
    const branch = defaultBranch.trim();
    const base = defaultBase.trim();
    const profile = effectiveDefaultProfile;
    const seeded = reposWithoutBlueprint.map<SessionBlueprint>((repoPath) => ({
      id: genId(),
      name: renderTemplate(nameTemplate, repoPath) || repoHandle(repoPath),
      repoRoot: repoPath,
      branch: branch || null,
      worktreePath: null,
      spawnProfile: profile,
      base: base || null,
      fetchFirst: defaultFetchFirst,
    }));
    blueprints = [...blueprints, ...seeded];
    error = "";
  }

  function genId(): string {
    if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
      return crypto.randomUUID();
    }
    return Math.random().toString(36).slice(2) + Date.now().toString(36);
  }

  async function pickFile(): Promise<string | null> {
    try {
      const result = await open({ multiple: false });
      return typeof result === "string" ? result : null;
    } catch (e) {
      logError(`new-project: file pick failed — ${e}`);
      return null;
    }
  }

  function removeRepoRoot(p: string) {
    repoRoots = repoRoots.filter((r) => r !== p);
    // Drop any blueprint that pointed at the removed repo so we don't ship
    // a dangling reference to the backend. Same goes for its template flag.
    const dropped = blueprints.filter((bp) => bp.repoRoot === p);
    blueprints = blueprints.filter((bp) => bp.repoRoot !== p);
    for (const bp of dropped) dropTemplateFlag(bp.id);
  }

  async function addContextPath() {
    const trimmed = newPathDraft.trim();
    if (trimmed) {
      if (!contextPaths.includes(trimmed))
        contextPaths = [...contextPaths, trimmed];
      newPathDraft = "";
      return;
    }
    const picked = await pickFile();
    if (picked && !contextPaths.includes(picked))
      contextPaths = [...contextPaths, picked];
  }

  function removeContextPath(p: string) {
    contextPaths = contextPaths.filter((cp) => cp !== p);
  }

  function addBlueprint() {
    if (repoRoots.length === 0) {
      error = "Add at least one repo root before configuring sessions";
      return;
    }
    const id = genId();
    blueprints = [
      ...blueprints,
      {
        id,
        name: "",
        repoRoot: repoRoots[0],
        branch: null,
        worktreePath: null,
        spawnProfile: defaultProfileId,
        base: null,
        fetchFirst: false,
      },
    ];
  }

  function removeBlueprint(id: string) {
    blueprints = blueprints.filter((bp) => bp.id !== id);
    dropTemplateFlag(id);
  }

  function updateBlueprint(id: string, patch: Partial<SessionBlueprint>) {
    blueprints = blueprints.map((bp) =>
      bp.id === id ? { ...bp, ...patch } : bp,
    );
  }

  async function previewProjectPrompt() {
    if (!projectPrompt.trim()) {
      promptPreview = "";
      promptPreviewError = "";
      return;
    }

    const blueprint = selectedPreviewBlueprint;
    const profileId = blueprint?.spawnProfile ?? effectiveDefaultProfile;
    const profile = profiles.find((p) => p.id === profileId) ?? null;
    const context = buildProjectPromptPreviewContext({
      project: {
        id: project?.id ?? null,
        name: name.trim(),
        repoRoots,
        contextPaths,
      },
      blueprint,
      profile,
      settings: $settings,
      sessions: $sessionState.sessions,
    });

    promptPreviewing = true;
    promptPreviewError = "";
    try {
      promptPreview = await renderProjectPromptTemplate(projectPrompt, context);
    } catch (e) {
      promptPreview = "";
      promptPreviewError = e instanceof Error ? e.message : String(e);
    } finally {
      promptPreviewing = false;
    }
  }

  function validate(): string | null {
    if (!name.trim()) return "Project name is required";
    for (const bp of blueprints) {
      if (!bp.name.trim()) return "Every session needs a name";
      if (!bp.repoRoot) return `Session "${bp.name}" needs a repo`;
    }
    return null;
  }

  async function handleSave() {
    const err = validate();
    if (err) {
      error = err;
      return;
    }
    error = "";
    saving = true;
    try {
      const cleanedBlueprints = blueprints.map((bp) => ({
        ...bp,
        branch: bp.branch?.trim() || null,
        base: bp.base?.trim() || null,
        worktreePath: bp.worktreePath?.trim() || null,
      }));
      const trimmedPrompt = projectPrompt.trim();
      if (isEdit && project) {
        await updateProject(project.id, {
          name: name.trim(),
          repoRoots,
          contextPaths,
          sessionBlueprints: cleanedBlueprints,
          projectPrompt: trimmedPrompt,
        });
      } else {
        // Create-mode behavior: every row spawns as a live session. Rows
        // with the "save as template" flag *also* persist on the project
        // as a blueprint so they can be re-spawned later from the sidebar.
        const keptBlueprints = cleanedBlueprints.filter((bp) =>
          isKeptAsTemplate(bp.id),
        );
        const created = await createProjectFull(name.trim(), {
          repoRoots,
          contextPaths,
          sessionBlueprints: keptBlueprints,
          projectPrompt: trimmedPrompt,
        });
        // Spawn sequentially: parallel spawns starve the PTY backend and
        // make a single failure look like a cascade. If any single spawn
        // fails, log it and keep going so the rest of the set still lands.
        for (const bp of cleanedBlueprints) {
          try {
            await spawnBlueprintForProject(created, bp, {
              blueprintId: isKeptAsTemplate(bp.id) ? bp.id : null,
            });
          } catch (e) {
            logError(`new-project: spawn "${bp.name}" failed — ${e}`);
          }
        }
      }
      onclose();
    } catch (e) {
      error = String(e);
      logError(`new-project: save failed — ${e}`);
    } finally {
      saving = false;
    }
  }

  async function handleDelete() {
    if (!isEdit || !project) return;
    saving = true;
    error = "";
    try {
      await removeProject(project.id);
      onclose();
    } catch (e) {
      error = String(e);
      logError(`new-project: delete failed — ${e}`);
      confirmDelete = false;
    } finally {
      saving = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      handleSave();
    }
  }

  const sectionLabel =
    "text-[11px] font-semibold uppercase tracking-wider text-text-muted";
  const fieldLabel = "text-[10px] uppercase tracking-wider text-text-muted";
  const inputClass =
    "w-full rounded-md border border-border-subtle bg-bg-deep px-3 py-2 text-[13px] text-text-primary outline-none focus:border-accent-dim";
  const smallInput =
    "rounded-md border border-border-subtle bg-bg-deep px-2 py-1.5 text-[12px] text-text-primary outline-none focus:border-accent-dim disabled:opacity-50";
  const ghostBtn =
    "cursor-pointer rounded-md border border-border-subtle bg-bg-surface px-2.5 py-1.5 text-[11px] text-text-secondary hover:bg-bg-hover hover:text-text-primary";
  const chipRow =
    "group flex items-center justify-between gap-2 rounded-md border border-border-subtle/70 bg-bg-deep/70 px-2.5 py-1";
</script>

{#if visible}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-md"
    transition:fade={{ duration: 120 }}
    onkeydown={onKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div
      class="flex max-h-[90vh] w-[720px] max-w-[92vw] flex-col rounded-2xl border border-border bg-bg-surface shadow-xl"
      transition:scale={{ duration: 120, start: 0.97 }}
    >
      <div class="border-b border-hairline px-6 py-4">
        <h2 class="text-[15px] font-semibold text-text-primary">
          {isEdit ? "Edit project" : "New project"}
        </h2>
        <p class="mt-1 text-[12px] text-text-muted">
          Tag sessions across one or more repos. Set defaults once, then
          generate or hand-tune.
        </p>
      </div>

      <div class="app-scrollbar flex-1 overflow-y-auto px-6 py-5">
        <div class="flex flex-col gap-6">
          <!-- Name -->
          <div class="flex flex-col gap-1.5">
            <label for="np-name" class={sectionLabel}>Name</label>
            <input
              id="np-name"
              class={inputClass}
              bind:value={name}
              placeholder="my-feature"
              autocomplete="off"
            />
          </div>

          <!-- Repos -->
          <div class="flex flex-col gap-2">
            <label for="np-repo-picker" class={sectionLabel}>Repos</label>
            {#if repoRoots.length > 0}
              <ul class="flex flex-col gap-1">
                {#each repoRoots as root (root)}
                  <li class={chipRow}>
                    <span
                      class="truncate font-mono text-[12px] text-text-primary"
                      >{root}</span
                    >
                    <button
                      class="text-[11px] text-text-muted opacity-0 transition-opacity hover:text-red group-hover:opacity-100"
                      onclick={() => removeRepoRoot(root)}
                    >
                      remove
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            <RepoAutoComplete
              id="np-repo-picker"
              bind:value={repoDraft}
              options={repoOptions}
              placeholder="Type path or search configured repo roots"
              loading={discoveryLoading}
              {hasConfiguredRoots}
              showRefresh
              showBrowse
              onrefresh={loadDiscoveredRepos}
              onselect={(path) => pushRepoRoot(path)}
              onenter={(text) => pushRepoRoot(text)}
            />
            {#if !hasConfiguredRoots}
              <p class="text-[11px] text-text-muted">
                Configure repo roots in Settings to enable the quick-pick.
              </p>
            {:else if !discoveryLoading && discoveredRepos.length === 0}
              <p class="text-[11px] text-text-muted">
                No git repositories found under configured roots.
              </p>
            {/if}
          </div>

          <!-- Default session config + Generate -->
          <div
            class="flex flex-col gap-3 rounded-md border border-border-subtle bg-bg-deep/40 p-3"
          >
            <div class="flex items-baseline justify-between gap-3">
              <span class={sectionLabel}>Defaults</span>
              <span class="text-[10px] text-text-muted">
                tokens:
                <code
                  class="rounded bg-bg-deep px-1 py-0.5 font-mono text-[10px]"
                  >{`{{repo}}`}</code
                >
                <code
                  class="ml-1 rounded bg-bg-deep px-1 py-0.5 font-mono text-[10px]"
                  >{`{{branch}}`}</code
                >
                <code
                  class="ml-1 rounded bg-bg-deep px-1 py-0.5 font-mono text-[10px]"
                  >{`{{project}}`}</code
                >
              </span>
            </div>
            <div class="grid grid-cols-[1.4fr_1fr] gap-2">
              <label class="flex flex-col gap-1">
                <span class={fieldLabel}>name template</span>
                <input
                  class={smallInput}
                  bind:value={nameTemplate}
                  placeholder={namePlaceholder}
                />
              </label>
              <label class="flex flex-col gap-1">
                <span class={fieldLabel}>profile</span>
                <select class={smallInput} bind:value={defaultProfileChoice}>
                  <option value="">{defaultProfileId} (default)</option>
                  {#each profiles as p (p.id)}
                    <option value={p.id}>{p.name ?? p.id}</option>
                  {/each}
                </select>
              </label>
              <label class="flex flex-col gap-1">
                <span class={fieldLabel}>branch (worktree)</span>
                <input
                  class={smallInput}
                  bind:value={defaultBranch}
                  placeholder="(empty = no worktree)"
                />
              </label>
              <label class="flex flex-col gap-1">
                <span class={fieldLabel}>base ref</span>
                <input
                  class={smallInput}
                  bind:value={defaultBase}
                  placeholder="origin/main"
                  disabled={!defaultBranch.trim()}
                />
              </label>
              {#if defaultBranch.trim()}
                <label class="col-span-2 flex items-center gap-2">
                  <input type="checkbox" bind:checked={defaultFetchFirst} />
                  <span class="text-[12px] text-text-primary"
                    >fetch origin before resolving base</span
                  >
                </label>
              {/if}
            </div>
            <div
              class="flex items-center justify-between gap-3 border-t border-hairline pt-3"
            >
              <p class="min-w-0 flex-1 truncate text-[11px] text-text-muted">
                {#if reposWithoutBlueprint.length > 0}
                  {reposWithoutBlueprint.length} new session{reposWithoutBlueprint.length ===
                  1
                    ? ""
                    : "s"}:
                  <span class="font-mono text-text-secondary"
                    >{previewNames.join(", ")}</span
                  >
                {:else if repoRoots.length > 0}
                  Every repo already has a session — remove a row below to
                  regenerate.
                {:else}
                  Add a repo above to enable generation.
                {/if}
              </p>
              <button
                class="shrink-0 cursor-pointer rounded-md border border-accent-dim/20 bg-accent-dim/15 px-3 py-1.5 text-[11px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50"
                onclick={generateFromRepos}
                disabled={reposWithoutBlueprint.length === 0}
              >
                Generate
              </button>
            </div>
          </div>

          <!-- Session blueprints -->
          <div class="flex flex-col gap-2">
            <div class="flex items-baseline justify-between">
              <span class={sectionLabel}>Sessions</span>
              <span class="text-[10px] text-text-muted">
                {blueprints.length} configured · {isEdit
                  ? "edit templates"
                  : "spawned on create"}
              </span>
            </div>
            {#if blueprints.length > 0}
              <ul
                class="flex flex-col divide-y divide-hairline rounded-md border border-border-subtle bg-bg-deep/40"
              >
                {#each blueprints as bp (bp.id)}
                  <li class="flex flex-col gap-1.5 px-2.5 py-2">
                    <div class="flex items-center gap-2">
                      <input
                        class={smallInput + " min-w-0 flex-[1.4]"}
                        value={bp.name}
                        oninput={(e) =>
                          updateBlueprint(bp.id, {
                            name: e.currentTarget.value,
                          })}
                        placeholder="name"
                        aria-label="name"
                      />
                      <select
                        class={smallInput + " min-w-0 flex-[1.5]"}
                        value={bp.repoRoot}
                        onchange={(e) =>
                          updateBlueprint(bp.id, {
                            repoRoot: e.currentTarget.value,
                          })}
                        aria-label="repo"
                      >
                        {#each repoRoots as root (root)}
                          <option value={root}>{repoHandle(root)}</option>
                        {/each}
                      </select>
                      <input
                        class={smallInput + " min-w-0 flex-1"}
                        value={bp.branch ?? ""}
                        oninput={(e) =>
                          updateBlueprint(bp.id, {
                            branch: e.currentTarget.value,
                          })}
                        placeholder="branch"
                        aria-label="branch"
                      />
                      <select
                        class={smallInput + " min-w-0 flex-1"}
                        value={bp.spawnProfile}
                        onchange={(e) =>
                          updateBlueprint(bp.id, {
                            spawnProfile: e.currentTarget.value,
                          })}
                        aria-label="profile"
                      >
                        {#each profiles as p (p.id)}
                          <option value={p.id}>{p.name ?? p.id}</option>
                        {/each}
                      </select>
                      <button
                        class="shrink-0 text-[11px] text-text-muted hover:text-red"
                        onclick={() => removeBlueprint(bp.id)}
                        aria-label="remove session"
                      >
                        ×
                      </button>
                    </div>
                    {#if bp.branch}
                      <div
                        class="flex items-center gap-3 pl-1 text-[11px] text-text-muted"
                      >
                        <label class="flex items-center gap-1.5">
                          <span>base</span>
                          <input
                            class={smallInput + " w-44"}
                            value={bp.base ?? ""}
                            oninput={(e) =>
                              updateBlueprint(bp.id, {
                                base: e.currentTarget.value,
                              })}
                            placeholder="origin/main"
                          />
                        </label>
                        <label class="flex items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={bp.fetchFirst ?? false}
                            onchange={(e) =>
                              updateBlueprint(bp.id, {
                                fetchFirst: e.currentTarget.checked,
                              })}
                          />
                          <span>fetch first</span>
                        </label>
                      </div>
                    {/if}
                    {#if !isEdit}
                      <label
                        class="flex cursor-pointer items-center gap-1.5 pl-1 text-[10px] text-text-muted"
                      >
                        <input
                          type="checkbox"
                          checked={isKeptAsTemplate(bp.id)}
                          onchange={(e) =>
                            setKeepAsTemplate(bp.id, e.currentTarget.checked)}
                        />
                        <span
                          >also save as template (re-spawn later from sidebar)</span
                        >
                      </label>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            <button class={ghostBtn + " self-start"} onclick={addBlueprint}
              >+ Add session</button
            >
          </div>

          <!-- Project prompt -->
          <div class="flex flex-col gap-2">
            <div class="flex items-baseline justify-between gap-3">
              <label for="np-project-prompt" class={sectionLabel}
                >Project prompt</label
              >
              <span class="text-[10px] text-text-muted">
                appended via <code
                  class="rounded bg-bg-deep px-1 py-0.5 font-mono text-[10px]"
                  >--append-system-prompt</code
                >
                (Claude) /
                <code
                  class="ml-1 rounded bg-bg-deep px-1 py-0.5 font-mono text-[10px]"
                  >-c instructions=…</code
                > (Codex)
              </span>
            </div>
            <textarea
              id="np-project-prompt"
              class={inputClass +
                " min-h-[88px] resize-y font-mono text-[12px]"}
              bind:value={projectPrompt}
              placeholder="Extra instructions to inject at the top of every spawned agent's system prompt"
              rows="4"
            ></textarea>
            {#if projectPrompt.trim()}
              <div
                class="rounded-md border border-border-subtle/70 bg-bg-deep/60 p-2"
              >
                <div class="flex flex-wrap items-center gap-2">
                  {#if blueprints.length > 1}
                    <select
                      class={smallInput + " max-w-[240px]"}
                      bind:value={promptPreviewBlueprintId}
                      disabled={promptPreviewing}
                    >
                      {#each blueprints as bp (bp.id)}
                        <option value={bp.id}
                          >{bp.name || "Untitled session"}</option
                        >
                      {/each}
                    </select>
                  {:else if selectedPreviewBlueprint}
                    <span class="truncate text-[11px] text-text-muted">
                      Preview: {selectedPreviewBlueprint.name ||
                        "Untitled session"}
                    </span>
                  {:else}
                    <span class="truncate text-[11px] text-text-muted"
                      >Preview: draft project</span
                    >
                  {/if}
                  <button
                    class={ghostBtn}
                    onclick={previewProjectPrompt}
                    disabled={promptPreviewing}
                  >
                    {promptPreviewing ? "Previewing…" : "Preview"}
                  </button>
                  <span class="text-[11px] text-text-muted"
                    >Minijinja variables</span
                  >
                </div>
                {#if promptPreviewError}
                  <p class="mt-2 whitespace-pre-wrap text-[11px] text-red">
                    {promptPreviewError}
                  </p>
                {/if}
                {#if promptPreview}
                  <pre
                    class="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-md border border-border-subtle/60 bg-bg-surface/60 p-2 font-mono text-[11px] leading-relaxed text-text-primary">{promptPreview}</pre>
                {/if}
              </div>
            {/if}
          </div>

          <!-- Context paths -->
          <div class="flex flex-col gap-2">
            <div class="flex items-baseline justify-between gap-3">
              <span class={sectionLabel}>Context paths</span>
              <span class="text-[10px] text-text-muted">
                exposed as <code
                  class="rounded bg-bg-deep px-1 py-0.5 font-mono text-[10px]"
                  >$ROUX_PROJECT_CONTEXT_PATHS</code
                >
              </span>
            </div>
            {#if contextPaths.length > 0}
              <ul class="flex flex-col gap-1">
                {#each contextPaths as p (p)}
                  <li class={chipRow}>
                    <span
                      class="truncate font-mono text-[12px] text-text-primary"
                      >{p}</span
                    >
                    <button
                      class="text-[11px] text-text-muted opacity-0 transition-opacity hover:text-red group-hover:opacity-100"
                      onclick={() => removeContextPath(p)}
                    >
                      remove
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            <div class="flex items-center gap-2">
              <input
                class={smallInput + " flex-1"}
                bind:value={newPathDraft}
                placeholder="Paste a file path or pick…"
                onkeydown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    addContextPath();
                  }
                }}
              />
              <button class={ghostBtn} onclick={addContextPath}>+ Add</button>
            </div>
          </div>

          {#if error}
            <p class="text-xs text-red">{error}</p>
          {/if}
        </div>
      </div>

      <div class="flex justify-end gap-2 border-t border-hairline px-6 py-4">
        {#if isEdit && confirmDelete}
          <div class="mr-auto flex items-center gap-2">
            <span class="text-[11px] text-text-muted">
              Delete? Sessions stay (just untagged).
            </span>
            <button
              class="cursor-pointer rounded-xl border border-red/30 bg-red/15 px-3 py-1.5 text-[12px] font-medium text-red hover:bg-red/24 disabled:opacity-50"
              onclick={handleDelete}
              disabled={saving}
            >
              {saving ? "Deleting…" : "Delete"}
            </button>
            <button
              class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-3 py-1.5 text-[12px] text-text-secondary hover:bg-bg-hover"
              onclick={() => (confirmDelete = false)}
              disabled={saving}
            >
              Cancel
            </button>
          </div>
        {:else if isEdit}
          <button
            class="mr-auto cursor-pointer self-center text-[12px] font-medium text-red/85 hover:text-red"
            onclick={() => (confirmDelete = true)}
          >
            Delete project
          </button>
        {:else}
          <div class="mr-auto self-center text-[11px] text-text-muted">
            Esc to close • Cmd/Ctrl+Enter to save
          </div>
        {/if}
        <button
          class="cursor-pointer rounded-xl border border-border-subtle bg-bg-surface px-5 py-2 text-[13px] font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary"
          onclick={onclose}
        >
          Cancel
        </button>
        <button
          class="cursor-pointer rounded-xl border border-accent-dim/20 bg-accent-dim/15 px-5 py-2 text-[13px] font-medium text-accent hover:bg-accent-dim/24 disabled:opacity-50"
          onclick={handleSave}
          disabled={saving}
        >
          {saving ? "Saving…" : isEdit ? "Save" : "Create"}
        </button>
      </div>
    </div>
  </div>
{/if}
