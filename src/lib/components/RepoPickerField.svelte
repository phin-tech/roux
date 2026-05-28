<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { settings } from "$lib/stores/settings";
  import { buildQuickPickOptions } from "$lib/repos/quickPick";
  import RepoAutoComplete from "./RepoAutoComplete.svelte";

  interface Props {
    value: string;
    id?: string;
    label?: string | null;
    placeholder?: string;
    enabled?: boolean;
    showRefresh?: boolean;
    showBrowse?: boolean;
    refreshLabel?: string;
    browseLabel?: string;
    emptyText?: string;
    noReposText?: string;
    onselect?: (path: string, label: string) => void | Promise<void>;
    onenter?: (text: string) => void | Promise<void>;
    onbrowse?: () => void | Promise<void>;
    onrepos?: (paths: string[]) => void;
  }

  let {
    value = $bindable(""),
    id,
    label = "Repository",
    placeholder = "Type path or search configured repo roots",
    enabled = true,
    showRefresh = true,
    showBrowse = true,
    refreshLabel = "Refresh",
    browseLabel = "Browse",
    emptyText = "No matching repositories",
    noReposText = "No git repositories found under configured roots.",
    onselect,
    onenter,
    onbrowse,
    onrepos,
  }: Props = $props();

  let repoPaths = $state<string[]>([]);
  let loading = $state(false);
  let error = $state("");

  const compatSettings = $derived.by<{
    repoRoots: string[];
    excludeWorktreesFromRepoRoots: boolean;
  }>(() => {
    const raw = $settings as unknown as {
      repoRoots?: string[];
      excludeWorktreesFromRepoRoots?: boolean;
    };
    return {
      repoRoots: Array.isArray(raw.repoRoots) ? raw.repoRoots : [],
      excludeWorktreesFromRepoRoots:
        typeof raw.excludeWorktreesFromRepoRoots === "boolean"
          ? raw.excludeWorktreesFromRepoRoots
          : true,
    };
  });
  const hasConfiguredRoots = $derived(compatSettings.repoRoots.length > 0);
  const options = $derived.by(() => buildQuickPickOptions(repoPaths));

  $effect(() => {
    if (!enabled) return;
    const rootsKey = compatSettings.repoRoots.join("\n");
    const excludeWorktrees = compatSettings.excludeWorktreesFromRepoRoots;
    void rootsKey;
    void excludeWorktrees;
    void refreshRepos();
  });

  async function refreshRepos() {
    const roots = compatSettings.repoRoots.map((r) => r.trim()).filter(Boolean);
    if (roots.length === 0) {
      repoPaths = [];
      error = "";
      onrepos?.([]);
      return;
    }
    loading = true;
    error = "";
    try {
      const paths = await invoke<string[]>("list_git_repos_in_roots", {
        roots,
        excludeWorktrees: compatSettings.excludeWorktreesFromRepoRoots,
      });
      repoPaths = paths;
      onrepos?.(paths);
    } catch (e) {
      repoPaths = [];
      error = String(e);
      onrepos?.([]);
    } finally {
      loading = false;
    }
  }
</script>

<div class="flex flex-col gap-1.5">
  {#if label}
    <label
      for={id}
      class="text-[11px] font-semibold uppercase tracking-wider text-text-muted"
    >
      {label}
    </label>
  {/if}
  <RepoAutoComplete
    {id}
    bind:value
    {options}
    {placeholder}
    {loading}
    {hasConfiguredRoots}
    {showRefresh}
    {showBrowse}
    {refreshLabel}
    {browseLabel}
    {emptyText}
    onrefresh={() => {
      void refreshRepos();
    }}
    {onbrowse}
    onselect={(path, optionLabel) => {
      void onselect?.(path, optionLabel);
    }}
    onenter={(text) => {
      void onenter?.(text);
    }}
  />
  {#if error}
    <p class="text-[11px] text-red">{error}</p>
  {:else if hasConfiguredRoots && !loading && repoPaths.length === 0}
    <p class="text-[11px] text-text-muted">{noReposText}</p>
  {/if}
</div>
