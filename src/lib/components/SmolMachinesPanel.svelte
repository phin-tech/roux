<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import PinButton from "./PinButton.svelte";
  import SmolMachineRow from "./SmolMachineRow.svelte";
  import {
    appendWorktreeMount,
    checkWorktreeMount,
    createSmolMachine,
    deleteSmolMachine,
    installSmolvmAgent,
    installSmolvmAgentPersist,
    installSmolvmAgentRecreate,
    listSmolMachineSmolfiles,
    listSmolMachines,
    managedProxyStatus,
    openSmolvmBootstrapConfig,
    setSessionSmolMachine,
    startManagedProxy,
    startSmolMachine,
    stopManagedProxy,
    stopSmolMachine,
    type SmolvmPersistOutcome,
  } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";
  import type { ManagedProxyStatus } from "$lib/types";
  import type { SmolMachine } from "$lib/types";
  import { activeSession, sessionState } from "$lib/stores/sessions";
  import { smolvmDetection } from "$lib/stores/smolvmDetection";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Shield from "@lucide/svelte/icons/shield";
  import ShieldOff from "@lucide/svelte/icons/shield-off";
  import Sliders from "@lucide/svelte/icons/sliders-horizontal";
  import X from "@lucide/svelte/icons/x";

  interface Props {
    visible: boolean;
    onclose: () => void;
    pinned?: boolean;
    onTogglePin?: () => void;
  }

  let { visible, onclose, pinned = false, onTogglePin }: Props = $props();

  let machines = $state<SmolMachine[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  // Per-machine "an action is in flight" set so we can disable that row's
  // buttons without freezing the whole panel.
  let busyNames = $state<Set<string>>(new Set());

  // Map of `{ machineName: smolfilePath }` for machines created with a
  // Smolfile (or recreated via the persist flow). Refreshed alongside
  // the machines list. Used to drive the row's "edit linked file" vs.
  // "create + recreate" hint and to skip the recreate-confirm modal
  // when a link already exists.
  let smolfileLinks = $state<Record<string, string>>({});

  // Pending recreate-confirm state — when set, a modal renders showing
  // exactly what's about to happen.
  let pendingRecreate = $state<
    | {
        machineName: string;
        agent: "claude" | "codex";
        proposedSmolfilePath: string;
        image: string | null;
        script: string;
      }
    | null
  >(null);

  // Phase 2.9: post-bind banner state. When the assigned machine's
  // linked Smolfile doesn't mount the session's worktree, surface a
  // one-click "Add same-path mount" affordance. Cleared once the user
  // dismisses or appends.
  let pendingMountPrompt = $state<
    | {
        machineName: string;
        worktreePath: string;
        smolfilePath: string;
        proposedSpec: string;
      }
    | null
  >(null);
  let mountAppendBusy = $state(false);

  // --- Create-machine inline form state -------------------------------
  let createOpen = $state(false);
  let newName = $state("");
  let newSmolfile = $state("");
  let newImage = $state("");
  let newNetwork = $state(false);
  let newSshAgent = $state(false);
  let newProxyUrl = $state("");
  // Phase 2.9: each row is one host:guest[:ro] mount the user wants
  // baked into `smolvm machine create -v ...`. Empty rows are dropped
  // at submit time. Same-path mounting is the common case so we
  // pre-fill `guest` from `host` when only host is set.
  let newVolumes = $state<{ host: string; guest: string; ro: boolean }[]>(
    [],
  );
  let creating = $state(false);
  let createError = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);

  function addVolumeRow(): void {
    newVolumes = [...newVolumes, { host: "", guest: "", ro: false }];
  }

  function removeVolumeRow(index: number): void {
    newVolumes = newVolumes.filter((_, i) => i !== index);
  }

  // Compose the host:guest[:ro] specs that ship to the backend.
  // - Drops rows where `host` is blank.
  // - Defaults `guest` to `host` (same-path) when blank — this is the
  //   case that makes `--workdir <host_worktree>` "just work" inside
  //   the guest.
  function composeVolumeSpecs(): string[] {
    return newVolumes
      .map((row) => {
        const host = row.host.trim();
        if (!host) return null;
        const guest = row.guest.trim() || host;
        return `${host}:${guest}${row.ro ? ":ro" : ""}`;
      })
      .filter((v): v is string => v !== null);
  }

  async function browseVolumeHostPath(index: number): Promise<void> {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    newVolumes = newVolumes.map((row, i) =>
      i === index ? { ...row, host: picked } : row,
    );
  }

  // --- Managed proxy state --------------------------------------------
  // Reflects the live state of the user-configured host proxy. Polled
  // when the panel is visible so the chip + start/stop button stay in
  // sync if the proxy crashes externally.
  let proxyStatus = $state<ManagedProxyStatus>({
    running: false,
    port: null,
    bind: null,
    pid: null,
    lastError: null,
  });
  let proxyBusy = $state(false);
  let proxyError = $state<string | null>(null);

  async function refreshProxyStatus(): Promise<void> {
    try {
      proxyStatus = await managedProxyStatus();
    } catch (err) {
      proxyError = typeof err === "string" ? err : String(err);
    }
  }

  $effect(() => {
    if (!visible) return;
    void refreshProxyStatus();
    // Poll every 5s so an externally-killed proxy reflects in the UI.
    // Cheap — single Tauri command, no I/O on the host beyond a
    // mutex read in services::managed_proxy::status.
    const id = window.setInterval(() => void refreshProxyStatus(), 5000);
    return () => window.clearInterval(id);
  });

  async function handleStartProxy(): Promise<void> {
    proxyBusy = true;
    proxyError = null;
    try {
      proxyStatus = await startManagedProxy();
      // Auto-fill the create form's proxy URL field if it's empty,
      // so users aren't left typing the URL Roux just bound.
      if (!newProxyUrl.trim() && proxyStatus.running && proxyStatus.port) {
        newProxyUrl = `http://${proxyStatus.bind ?? "127.0.0.1"}:${proxyStatus.port}`;
      }
    } catch (err) {
      proxyError = typeof err === "string" ? err : String(err);
    } finally {
      proxyBusy = false;
    }
  }

  async function handleStopProxy(): Promise<void> {
    proxyBusy = true;
    proxyError = null;
    try {
      proxyStatus = await stopManagedProxy();
    } catch (err) {
      proxyError = typeof err === "string" ? err : String(err);
    } finally {
      proxyBusy = false;
    }
  }

  // Autofocus the Name field whenever the form opens. Tied to the open
  // flag rather than mount so re-opening the form re-focuses.
  $effect(() => {
    if (createOpen && nameInput) {
      nameInput.focus();
    }
  });

  // The Smolfile is authoritative when set — image/network inputs hide
  // because smolvm reads those from the file. Per the integration plan
  // decision (clarification #4).
  let smolfileSet = $derived(newSmolfile.trim().length > 0);

  function resetCreateForm(): void {
    newName = "";
    newSmolfile = "";
    newImage = "";
    newNetwork = false;
    newSshAgent = false;
    newProxyUrl = "";
    newVolumes = [];
    createError = null;
  }

  function closeCreateForm(): void {
    createOpen = false;
    resetCreateForm();
  }

  async function handleBrowseSmolfile(): Promise<void> {
    try {
      // Filter for .toml so a Smolfile.toml is easy to find, but also
      // show "All files" so users can pick a literal `Smolfile` (no
      // extension) if that's how their repo organizes it.
      const result = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "Smolfile", extensions: ["toml"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof result === "string") {
        newSmolfile = result;
      }
    } catch (err) {
      createError = `File picker failed: ${err}`;
    }
  }

  async function handleCreate(): Promise<void> {
    const name = newName.trim();
    if (!name) {
      createError = "Name is required";
      return;
    }
    creating = true;
    createError = null;
    try {
      const smolfile = newSmolfile.trim();
      const image = newImage.trim();
      await createSmolMachine({
        name,
        smolfilePath: smolfile ? smolfile : null,
        // When a Smolfile is set, don't forward image/network — the form
        // hides those inputs but their state may be stale from a prior
        // open without a Smolfile, and smolvm rejects redundant flags.
        image: smolfile || !image ? null : image,
        network: smolfile ? false : newNetwork,
        sshAgent: smolfile ? false : newSshAgent,
        // Proxy URL only applies when no Smolfile is provided; when
        // a user picks their own Smolfile, that file is authoritative.
        hostProxyUrl: smolfile
          ? null
          : newProxyUrl.trim() || null,
        // Mount specs only applied when there's no Smolfile (same
        // reasoning — Smolfile [dev].volumes is authoritative there).
        volumes: smolfile ? undefined : composeVolumeSpecs(),
      });
      closeCreateForm();
      await loadMachines();
    } catch (err) {
      createError = typeof err === "string" ? err : String(err);
    } finally {
      creating = false;
    }
  }

  function onNameKeyDown(e: KeyboardEvent): void {
    if (e.key === "Enter" && !creating) {
      e.preventDefault();
      void handleCreate();
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeCreateForm();
    }
  }
  // --------------------------------------------------------------------

  async function loadMachines(): Promise<void> {
    if (!$smolvmDetection.binaryPath) {
      machines = [];
      smolfileLinks = {};
      return;
    }
    loading = true;
    error = null;
    try {
      const [list, links] = await Promise.all([
        listSmolMachines(),
        listSmolMachineSmolfiles(),
      ]);
      machines = list;
      smolfileLinks = links;
    } catch (err) {
      error = typeof err === "string" ? err : String(err);
      machines = [];
      smolfileLinks = {};
    } finally {
      loading = false;
    }
  }

  // Reload whenever the panel becomes visible or smolvm gets installed/
  // uninstalled mid-session (override change). Don't probe in the
  // background while hidden — the panel is the only consumer of this
  // list, so there's nothing to keep warm.
  $effect(() => {
    if (visible && $smolvmDetection.binaryPath) {
      void loadMachines();
    }
  });

  function setBusy(name: string, busy: boolean): void {
    const next = new Set(busyNames);
    if (busy) next.add(name);
    else next.delete(name);
    busyNames = next;
  }

  async function withBusy(name: string, fn: () => Promise<void>): Promise<void> {
    setBusy(name, true);
    error = null;
    try {
      await fn();
      await loadMachines();
    } catch (err) {
      error = typeof err === "string" ? err : String(err);
    } finally {
      setBusy(name, false);
    }
  }

  async function handleStart(name: string): Promise<void> {
    await withBusy(name, () => startSmolMachine(name));
  }

  async function handleStop(name: string): Promise<void> {
    await withBusy(name, () => stopSmolMachine(name));
  }

  async function handleDelete(name: string): Promise<void> {
    // Guarded by a native confirm — destructive, easy to misclick a row.
    const ok = window.confirm(`Delete smol machine "${name}"? This cannot be undone.`);
    if (!ok) return;
    await withBusy(name, () => deleteSmolMachine(name));
  }

  async function handleOpenBootstrapConfig(): Promise<void> {
    error = null;
    try {
      await openSmolvmBootstrapConfig();
    } catch (err) {
      error = `Could not open bootstrap config: ${typeof err === "string" ? err : String(err)}`;
    }
  }

  async function handleInstallAgent(
    name: string,
    agent: "claude" | "codex",
    mode: "run" | "persist",
  ): Promise<void> {
    const label = agent === "claude" ? "Claude" : "Codex";
    if (mode === "run") {
      const ok = window.confirm(
        `Install ${label} in smol machine "${name}"?\n\n` +
          `Runs a distro-aware install script inside the VM via\n` +
          `  smolvm machine exec --name ${name} -- sh -c "<script>"\n\n` +
          `This install is ephemeral — recreating the machine wipes it.\n` +
          `Use "Persist via Smolfile" to make it stick.`,
      );
      if (!ok) return;
      setBusy(name, true);
      error = null;
      try {
        await installSmolvmAgent(name, agent);
      } catch (err) {
        error = `Install ${label} failed: ${typeof err === "string" ? err : String(err)}`;
      } finally {
        setBusy(name, false);
      }
      return;
    }

    // mode === "persist"
    setBusy(name, true);
    error = null;
    let outcome: SmolvmPersistOutcome;
    try {
      outcome = await installSmolvmAgentPersist(name, agent);
    } catch (err) {
      error = `Persist ${label} failed: ${typeof err === "string" ? err : String(err)}`;
      setBusy(name, false);
      return;
    }
    setBusy(name, false);

    if (outcome.kind === "appended") {
      error = null;
      // Reload so the row picks up any new link state.
      await loadMachines();
      window.alert(
        `${label} install line appended to ${outcome.smolfilePath}.\n\n` +
          `It will run on every machine start (boot-time provisioning).\n` +
          `Recreate the machine — or simply restart it — to apply now.`,
      );
    } else if (outcome.kind === "alreadyPresent") {
      window.alert(
        `${label}'s install line is already present in ${outcome.smolfilePath}. ` +
          `No changes made.`,
      );
    } else if (outcome.kind === "needsRecreate") {
      // Stash the proposal and render the modal — user confirms there
      // before any destructive action runs.
      pendingRecreate = {
        machineName: name,
        agent,
        proposedSmolfilePath: outcome.proposedSmolfilePath,
        image: outcome.image,
        script: outcome.script,
      };
    }
  }

  async function handleConfirmRecreate(): Promise<void> {
    const r = pendingRecreate;
    if (!r) return;
    pendingRecreate = null;
    setBusy(r.machineName, true);
    error = null;
    try {
      await installSmolvmAgentRecreate(r.machineName, r.agent);
      await loadMachines();
    } catch (err) {
      error = `Recreate failed: ${typeof err === "string" ? err : String(err)}`;
    } finally {
      setBusy(r.machineName, false);
    }
  }

  async function handleAssign(name: string): Promise<void> {
    const session = $activeSession;
    if (!session) return;
    setBusy(name, true);
    error = null;
    // Clear any prompt left from a previous bind. A new assign always
    // owns the prompt slot — either we replace it with a fresh
    // notMounted result below, or the new machine is fine and the
    // prompt should disappear.
    pendingMountPrompt = null;
    try {
      await setSessionSmolMachine(session.id, name);
      // The backend persists the binding but does NOT emit a sessions
      // event the frontend listens to (same pattern as pinnedPrUrl —
      // see commands/watches.ts:205). Mirror the mutation into the
      // local store so the SessionCard badge and the row's bound-state
      // checkmark light up immediately.
      sessionState.update((state) => ({
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === session.id ? { ...s, smolMachineName: name } : s,
        ),
      }));

      // Phase 2.9 auto-mount detection. When the machine has a linked
      // Smolfile but no [dev].volumes entry covers the session's
      // worktree, surface the prompt so sessions don't silently land
      // in guest $HOME with --workdir failing. Quiet for machines
      // without a linked Smolfile — Roux can't mutate what it doesn't
      // own — and quiet for already-mounted worktrees.
      if (session.worktreePath) {
        try {
          const check = await checkWorktreeMount(name, session.worktreePath);
          if (check.kind === "notMounted") {
            pendingMountPrompt = {
              machineName: name,
              worktreePath: session.worktreePath,
              smolfilePath: check.smolfile_path,
              proposedSpec: check.proposed_spec,
            };
          }
          // mounted / noLinkedSmolfile → leave pendingMountPrompt at
          // null (already cleared above). Explicit no-op for
          // readability of the surrounding control flow.
        } catch (err) {
          // Non-fatal; bind succeeded. Log the check failure on the
          // panel error banner so the user knows the auto-mount UX
          // didn't run, but don't undo the bind.
          error = `bound, but mount check failed: ${typeof err === "string" ? err : String(err)}`;
        }
      }
    } catch (err) {
      error = typeof err === "string" ? err : String(err);
    } finally {
      setBusy(name, false);
    }
  }

  async function handleAppendMount(): Promise<void> {
    const p = pendingMountPrompt;
    if (!p) return;
    mountAppendBusy = true;
    try {
      const outcome = await appendWorktreeMount(
        p.machineName,
        p.proposedSpec,
      );
      pendingMountPrompt = null;
      if (outcome.kind === "appended") {
        window.alert(
          `Added \`${p.proposedSpec}\` to ${outcome.smolfilePath}.\n\n` +
            `Smolvm bakes volumes at machine create time, so recreate ` +
            `the machine (Smol Machines panel → Install Claude → ` +
            `Persist via Smolfile flow, or run ` +
            `\`smolvm machine delete ${p.machineName} && smolvm machine create ${p.machineName} -s ${outcome.smolfilePath}\`) ` +
            `to apply.`,
        );
      } else {
        window.alert(
          `\`${p.proposedSpec}\` was already in ${outcome.smolfilePath}. ` +
            `If sessions still land in guest $HOME, recreate the machine ` +
            `to pick up the existing entry.`,
        );
      }
    } catch (err) {
      error = `Could not append mount: ${typeof err === "string" ? err : String(err)}`;
    } finally {
      mountAppendBusy = false;
    }
  }
</script>

<div
  class="relative flex h-full w-full min-h-0 flex-col bg-bg-deep"
  class:hidden={!visible}
>
  <div
    class="flex h-9 shrink-0 items-center gap-2 border-b border-hairline bg-bg-surface/30 px-3"
  >
    <span class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
      Smol Machines
    </span>
    {#if $smolvmDetection.version}
      <span
        class="rounded bg-green/10 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-green"
      >
        {$smolvmDetection.version}
      </span>
    {/if}
    <div class="ml-auto flex items-center gap-1">
      {#if $smolvmDetection.binaryPath}
        <button
          type="button"
          class="flex h-6 w-6 items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary disabled:opacity-40 {createOpen ? 'bg-white/10 text-text-primary' : ''}"
          title={createOpen ? "Close create form" : "Create new machine"}
          aria-label={createOpen ? "Close create form" : "Create new machine"}
          aria-pressed={createOpen}
          onclick={() => {
            if (createOpen) {
              closeCreateForm();
            } else {
              createOpen = true;
            }
          }}
        >
          <Plus size={12} />
        </button>
      {/if}
      {#if $settings.managedProxy}
        <button
          type="button"
          class="flex h-6 items-center gap-1 rounded px-1.5 text-[10px] disabled:opacity-40 {proxyStatus.running
            ? 'bg-accent-dim/15 text-accent hover:bg-accent-dim/25'
            : 'text-text-muted hover:bg-bg-hover hover:text-text-primary'}"
          title={proxyStatus.running
            ? `Managed proxy running on ${proxyStatus.bind ?? '127.0.0.1'}:${proxyStatus.port}. Click to stop.`
            : proxyError ?? proxyStatus.lastError ?? 'Start the configured managed HTTP proxy.'}
          aria-label={proxyStatus.running ? 'Stop managed proxy' : 'Start managed proxy'}
          aria-pressed={proxyStatus.running}
          disabled={proxyBusy}
          onclick={() =>
            void (proxyStatus.running ? handleStopProxy() : handleStartProxy())}
        >
          {#if proxyStatus.running}
            <Shield size={11} />
            <span class="font-mono">{proxyStatus.bind ?? '127.0.0.1'}:{proxyStatus.port}</span>
          {:else}
            <ShieldOff size={11} />
            <span>{proxyError || proxyStatus.lastError ? 'proxy error' : 'proxy off'}</span>
          {/if}
        </button>
      {/if}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
        title="Refresh machines"
        aria-label="Refresh machines"
        disabled={loading || !$smolvmDetection.binaryPath}
        onclick={() => void loadMachines()}
      >
        <RefreshCw size={12} class={loading ? "animate-spin" : ""} />
      </button>
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-muted hover:bg-bg-hover hover:text-text-primary"
        title="Edit bootstrap config — customize prereqs and install commands per agent / distro"
        aria-label="Edit bootstrap config"
        onclick={() => void handleOpenBootstrapConfig()}
      >
        <Sliders size={12} />
      </button>
      {#if onTogglePin}
        <PinButton {pinned} ontoggle={onTogglePin} />
      {/if}
      <button
        class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1.5 text-base text-text-muted hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
        onclick={onclose}
        aria-label="Close smol machines panel">&times;</button
      >
    </div>
  </div>

  <div class="flex flex-1 min-h-0 flex-col overflow-auto">
    {#if !$smolvmDetection.binaryPath}
      <div
        class="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-sm text-text-secondary"
      >
        <p>smolvm is not installed.</p>
        <p class="text-[11px] text-text-muted">
          Install with:
        </p>
        <code
          class="rounded bg-bg-surface px-2 py-1 text-[11px] text-text-primary"
        >
          curl -sSL https://smolmachines.com/install.sh | bash
        </code>
        <p class="text-[11px] text-text-muted">
          Then restart Roux or update the smolvm binary path in settings.
        </p>
      </div>
    {:else}
      {#if createOpen}
        <div
          class="mx-2 mt-2 mb-2 rounded border border-accent-dim/40 bg-bg-surface/40 p-2"
        >
          <label
            class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
            for="smol-new-name">Name</label
          >
          <input
            id="smol-new-name"
            bind:this={nameInput}
            bind:value={newName}
            placeholder="my-vm"
            class="mb-2 w-full rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
            onkeydown={onNameKeyDown}
            disabled={creating}
          />

          <label
            class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
            for="smol-new-smolfile">Smolfile (optional)</label
          >
          <div class="mb-2 flex gap-1">
            <input
              id="smol-new-smolfile"
              bind:value={newSmolfile}
              placeholder="/path/to/Smolfile"
              class="min-w-0 flex-1 rounded border border-border bg-bg-deep px-2 py-1 font-mono text-[10px] text-text-primary outline-none focus:border-accent-dim"
              disabled={creating}
            />
            <button
              type="button"
              class="cursor-pointer rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:bg-bg-hover disabled:opacity-40"
              onclick={() => void handleBrowseSmolfile()}
              disabled={creating}
            >
              Browse
            </button>
          </div>

          {#if !smolfileSet}
            <label
              class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              for="smol-new-image">Image (optional)</label
            >
            <input
              id="smol-new-image"
              bind:value={newImage}
              placeholder="alpine"
              class="mb-2 w-full rounded border border-border bg-bg-deep px-2 py-1 font-mono text-xs text-text-primary outline-none focus:border-accent-dim"
              disabled={creating}
            />

            <label class="mb-2 flex cursor-pointer items-center gap-2 text-[11px] text-text-secondary">
              <input
                type="checkbox"
                bind:checked={newNetwork}
                class="h-3 w-3"
                disabled={creating}
              />
              Enable network
            </label>

            <label
              class="mb-2 flex cursor-pointer items-start gap-2 text-[11px] text-text-secondary"
              title="Forward your host's SSH agent into the VM so `git clone git@…` works for private repos. Private keys never leave the host — the hypervisor enforces it."
            >
              <input
                type="checkbox"
                bind:checked={newSshAgent}
                class="mt-0.5 h-3 w-3"
                disabled={creating}
              />
              <span>
                Forward SSH agent
                <span class="block text-[10px] text-text-muted">
                  Enables `git clone git@…` inside the VM. Requires
                  <code class="rounded bg-bg-surface px-1">ssh-add -l</code>
                  to list keys on the host.
                </span>
              </span>
            </label>

            <label
              class="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              for="smol-new-proxy"
            >
              Host HTTP proxy URL (optional)
            </label>
            <input
              id="smol-new-proxy"
              bind:value={newProxyUrl}
              placeholder={proxyStatus.running && proxyStatus.port
                ? `http://${proxyStatus.bind ?? "127.0.0.1"}:${proxyStatus.port}`
                : "http://192.168.64.1:8888"}
              class="mb-1 w-full rounded border border-border bg-bg-deep px-2 py-1 font-mono text-[10px] text-text-primary outline-none focus:border-accent-dim"
              disabled={creating}
            />
            <p class="mb-2 text-[10px] text-text-muted">
              Routes guest HTTP(S) through a host-side proxy. Useful when
              private registries IP-allowlist your host. Roux generates a
              managed Smolfile with `[dev].init` exporting `HTTP_PROXY` /
              `HTTPS_PROXY` so all login shells in the VM pick it up.
              {#if proxyStatus.running}
                Click the field to use the running managed proxy URL.
              {/if}
            </p>

            <div class="mb-1 flex items-center justify-between">
              <span
                class="block text-[10px] font-semibold uppercase tracking-wider text-text-muted"
              >
                Mount paths (optional)
              </span>
              <button
                type="button"
                class="rounded px-1.5 py-0.5 text-[10px] text-text-muted hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
                disabled={creating}
                onclick={addVolumeRow}
              >
                + Add mount
              </button>
            </div>
            {#each newVolumes as row, i (i)}
              <div class="mb-1 flex items-center gap-1">
                <input
                  bind:value={row.host}
                  placeholder="host path (e.g. /Users/me/code/foo)"
                  class="flex-1 rounded border border-border bg-bg-deep px-2 py-1 font-mono text-[10px] text-text-primary outline-none focus:border-accent-dim"
                  disabled={creating}
                />
                <button
                  type="button"
                  class="rounded px-1.5 py-1 text-[10px] text-text-muted hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
                  title="Browse for host directory"
                  disabled={creating}
                  onclick={() => void browseVolumeHostPath(i)}
                >
                  …
                </button>
                <input
                  bind:value={row.guest}
                  placeholder="guest path (defaults to host)"
                  class="flex-1 rounded border border-border bg-bg-deep px-2 py-1 font-mono text-[10px] text-text-primary outline-none focus:border-accent-dim"
                  disabled={creating}
                />
                <label
                  class="flex items-center gap-1 text-[10px] text-text-secondary"
                  title="Mount read-only"
                >
                  <input
                    type="checkbox"
                    bind:checked={row.ro}
                    class="h-3 w-3"
                    disabled={creating}
                  />
                  ro
                </label>
                <button
                  type="button"
                  class="rounded px-1 py-1 text-[10px] text-text-muted hover:bg-bg-hover hover:text-red disabled:opacity-40"
                  title="Remove mount"
                  disabled={creating}
                  onclick={() => removeVolumeRow(i)}
                >
                  <X size={10} />
                </button>
              </div>
            {/each}
            {#if newVolumes.length > 0}
              <p class="mb-2 text-[10px] text-text-muted">
                Same-path mounts (host == guest) let
                <code class="rounded bg-bg-surface px-1">--workdir</code>
                resolve to your worktree inside the VM. Leave guest blank
                to default to host.
              </p>
            {:else}
              <p class="mb-2 text-[10px] text-text-muted">
                Add a mount to expose host paths inside the VM (e.g. your
                worktree, so sessions land there instead of guest $HOME).
              </p>
            {/if}
          {:else}
            <p class="mb-2 text-[10px] text-text-muted">
              Image, network, SSH agent, and proxy env are read from the
              Smolfile (set <code class="rounded bg-bg-surface px-1">ssh_agent = true</code>
              and add an `HTTP_PROXY` export under
              <code class="rounded bg-bg-surface px-1">[dev].init</code>).
            </p>
          {/if}

          {#if createError}
            <div
              class="mb-2 flex items-start gap-2 rounded border border-red/30 bg-red/10 px-2 py-1 text-[11px] text-red"
            >
              <span class="min-w-0 flex-1 break-words">{createError}</span>
              <button
                type="button"
                class="shrink-0 text-red/80 hover:text-red"
                onclick={() => (createError = null)}
                aria-label="Dismiss error"
              >
                <X size={11} />
              </button>
            </div>
          {/if}

          <div class="flex justify-end gap-2">
            <button
              type="button"
              class="cursor-pointer rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:bg-bg-hover disabled:opacity-40"
              onclick={closeCreateForm}
              disabled={creating}>Cancel</button
            >
            <button
              type="button"
              class="cursor-pointer rounded border border-accent bg-accent-dim/20 px-2 py-0.5 text-[10px] text-accent hover:bg-accent-dim/40 disabled:opacity-40"
              onclick={() => void handleCreate()}
              disabled={creating || !newName.trim()}
              >{creating ? "Creating…" : "Create"}</button
            >
          </div>
        </div>
      {/if}

      {#if pendingMountPrompt}
        <div
          class="mx-2 mb-2 rounded border border-yellow/30 bg-yellow/10 px-2 py-2 text-[11px] text-text-primary"
        >
          <p class="mb-1 font-semibold text-yellow">
            Worktree not mounted in {pendingMountPrompt.machineName}
          </p>
          <p class="mb-2 text-text-secondary">
            Sessions bound to this machine will land in guest <code
              class="rounded bg-bg-surface px-1">$HOME</code
            >
            because <code class="rounded bg-bg-surface px-1 font-mono"
              >{pendingMountPrompt.worktreePath}</code
            > isn't covered by any
            <code class="rounded bg-bg-surface px-1">[dev].volumes</code> entry.
          </p>
          <p class="mb-2 text-text-muted">
            Roux can append a same-path mount
            (<code class="rounded bg-bg-surface px-1 font-mono"
              >{pendingMountPrompt.proposedSpec}</code
            >) to <code class="rounded bg-bg-surface px-1 font-mono"
              >{pendingMountPrompt.smolfilePath}</code
            >. Smolvm bakes volumes at create time, so you'll need to
            recreate the machine to apply.
          </p>
          <div class="flex justify-end gap-2">
            <button
              type="button"
              class="rounded border border-border-subtle bg-transparent px-2 py-1 text-text-muted hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
              disabled={mountAppendBusy}
              onclick={() => (pendingMountPrompt = null)}
            >
              Skip
            </button>
            <button
              type="button"
              class="rounded bg-yellow/20 px-2 py-1 font-semibold text-yellow hover:bg-yellow/30 disabled:opacity-40"
              disabled={mountAppendBusy}
              onclick={() => void handleAppendMount()}
            >
              Add mount
            </button>
          </div>
        </div>
      {/if}

      {#if error}
        <div class="px-3 py-3 text-[11px] text-red">
          {error}
        </div>
      {:else if loading && machines.length === 0}
        <div
          class="flex flex-1 items-center justify-center text-[11px] text-text-muted"
        >
          Loading machines…
        </div>
      {:else if machines.length === 0 && !createOpen}
        <div
          class="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center text-[11px] text-text-muted"
        >
          <p>No smol machines yet.</p>
          <p>
            Click <Plus size={11} class="inline" /> above to create one, or use
            <code class="rounded bg-bg-surface px-1.5 py-0.5 text-text-primary"
              >smolvm machine create</code
            >.
          </p>
        </div>
      {:else}
        {#each machines as machine (machine.name)}
          <SmolMachineRow
            {machine}
            busy={busyNames.has(machine.name)}
            boundToActive={$activeSession?.smolMachineName === machine.name}
            hasActiveSession={!!$activeSession}
            hasSmolfileLinked={machine.name in smolfileLinks}
            onStart={() => void handleStart(machine.name)}
            onStop={() => void handleStop(machine.name)}
            onDelete={() => void handleDelete(machine.name)}
            onAssign={() => void handleAssign(machine.name)}
            onInstallAgent={(agent, mode) =>
              void handleInstallAgent(machine.name, agent, mode)}
          />
        {/each}
      {/if}
    {/if}
  </div>

  {#if pendingRecreate}
    <!--
      Recreate-confirm modal. Blocks the panel until the user picks an
      option — destructive op (delete + recreate machine) is gated
      behind this explicit confirm. Closes by clearing
      `pendingRecreate` from either button or the cancel.
    -->
    <div
      class="absolute inset-0 z-30 flex items-center justify-center bg-bg-deep/85 px-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="smol-recreate-title"
    >
      <div class="w-full max-w-md rounded-md border border-border-subtle bg-bg-elevated p-3 shadow-xl shadow-black/40">
        <h3
          id="smol-recreate-title"
          class="mb-2 text-[12px] font-semibold uppercase tracking-wider text-text-primary"
        >
          Persist {pendingRecreate.agent === "claude" ? "Claude" : "Codex"} via Smolfile
        </h3>
        <p class="mb-2 text-[11px] text-text-secondary">
          <code class="rounded bg-bg-surface px-1 text-text-primary">{pendingRecreate.machineName}</code>
          has no Smolfile linked. Roux can:
        </p>
        <ol class="mb-3 list-decimal pl-5 text-[11px] text-text-secondary">
          <li>
            Generate
            <code class="rounded bg-bg-surface px-1 font-mono text-text-primary"
              >{pendingRecreate.proposedSmolfilePath}</code
            >
            with image
            <code class="rounded bg-bg-surface px-1 font-mono text-text-primary"
              >{pendingRecreate.image ?? "(unknown)"}</code
            >.
          </li>
          <li>Stop and delete the existing machine.</li>
          <li>Recreate it from the new Smolfile and start it.</li>
        </ol>
        <div class="mb-3 rounded border border-border-subtle bg-bg-deep px-2 py-1.5 font-mono text-[10px] text-text-secondary">
          [dev]<br />
          init = [<br />
          &nbsp;&nbsp;"{pendingRecreate.script}"<br />
          ]
        </div>
        <div class="mb-3 rounded border border-red/30 bg-red/10 px-2 py-1.5 text-[10px] text-red/90">
          ⚠ Active sessions inside this machine will be killed when it's
          deleted. Roux session bindings reattach by name on the next
          pane spawn — open panes will show their dead-pane banner during
          the gap.
          <br /><br />
          ⚠ If recreate fails after delete, the Smolfile remains at the
          path above; recover with
          <code class="rounded bg-bg-deep px-1 font-mono">smolvm machine create {pendingRecreate.machineName} -s {pendingRecreate.proposedSmolfilePath}</code>.
        </div>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="cursor-pointer rounded border border-border-subtle bg-bg-elevated px-2 py-0.5 text-[10px] text-text-secondary hover:bg-bg-hover"
            onclick={() => (pendingRecreate = null)}
          >Cancel</button>
          <button
            type="button"
            class="cursor-pointer rounded border border-red bg-red/20 px-2 py-0.5 text-[10px] text-red hover:bg-red/40"
            onclick={() => void handleConfirmRecreate()}
          >Generate &amp; Recreate</button>
        </div>
      </div>
    </div>
  {/if}
</div>
