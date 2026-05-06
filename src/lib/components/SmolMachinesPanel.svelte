<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import PinButton from "./PinButton.svelte";
  import SmolMachineRow from "./SmolMachineRow.svelte";
  import {
    createSmolMachine,
    deleteSmolMachine,
    listSmolMachines,
    setSessionSmolMachine,
    startSmolMachine,
    stopSmolMachine,
  } from "$lib/tauri";
  import type { SmolMachine } from "$lib/types";
  import { activeSession, sessionState } from "$lib/stores/sessions";
  import { smolvmDetection } from "$lib/stores/smolvmDetection";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
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

  // --- Create-machine inline form state -------------------------------
  let createOpen = $state(false);
  let newName = $state("");
  let newSmolfile = $state("");
  let newImage = $state("");
  let newNetwork = $state(false);
  let creating = $state(false);
  let createError = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);

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
      return;
    }
    loading = true;
    error = null;
    try {
      machines = await listSmolMachines();
    } catch (err) {
      error = typeof err === "string" ? err : String(err);
      machines = [];
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

  async function handleAssign(name: string): Promise<void> {
    const session = $activeSession;
    if (!session) return;
    setBusy(name, true);
    error = null;
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
    } catch (err) {
      error = typeof err === "string" ? err : String(err);
    } finally {
      setBusy(name, false);
    }
  }
</script>

<div
  class="flex h-full w-full min-h-0 flex-col bg-bg-deep"
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
          {:else}
            <p class="mb-2 text-[10px] text-text-muted">
              Image and network are read from the Smolfile.
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
            onStart={() => void handleStart(machine.name)}
            onStop={() => void handleStop(machine.name)}
            onDelete={() => void handleDelete(machine.name)}
            onAssign={() => void handleAssign(machine.name)}
          />
        {/each}
      {/if}
    {/if}
  </div>
</div>
