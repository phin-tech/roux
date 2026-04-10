<script lang="ts">
  import type { Session, PermissionInfo, WatchOutcome } from "$lib/types";
  import { renameSignal } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { watchState, flashingSessions } from "$lib/stores/watches";
  import { unreadBySession } from "$lib/stores/notifications";

  interface Props {
    session: Session;
    active: boolean;
    onselect: () => void;
    onclose: () => void;
    onrename: (newName: string) => void;
    onreconnect: () => void;
    onapprove: () => void;
    onalways: () => void;
    ondeny: () => void;
    ondismiss: () => void;
    oncontextmenu?: (e: MouseEvent) => void;
  }

  let {
    session,
    active,
    onselect,
    onclose,
    onrename,
    onreconnect,
    onapprove,
    onalways,
    ondeny,
    ondismiss,
    oncontextmenu,
  }: Props = $props();

  function formatPermission(info: PermissionInfo): string {
    if (info.toolName === "Bash" && info.toolInput?.command) {
      return `Bash: ${info.toolInput.command}`;
    }
    if (info.toolName === "Read" && info.toolInput?.file_path) {
      return `Read: ${info.toolInput.file_path}`;
    }
    if (info.toolName === "Write" && info.toolInput?.file_path) {
      return `Write: ${info.toolInput.file_path}`;
    }
    if (info.toolName === "Edit" && info.toolInput?.file_path) {
      return `Edit: ${info.toolInput.file_path}`;
    }
    if (info.message) {
      return info.message;
    }
    return info.toolName || "Permission needed";
  }

  function pathLabel(path: string): string {
    const parts = path.split("/").filter(Boolean);
    return parts.length > 0 ? parts[parts.length - 1] : path;
  }

  let editing = $state(false);
  let editName = $state("");

  $effect(() => {
    if (!editing) {
      editName = session.name;
    }
  });

  // Listen for rename signal from command palette
  let lastSignal = $state($renameSignal);
  $effect(() => {
    if ($renameSignal !== lastSignal) {
      lastSignal = $renameSignal;
      if (active) {
        editName = session.name;
        editing = true;
      }
    }
  });

  function startEditing(e: MouseEvent) {
    e.stopPropagation();
    editName = session.name;
    editing = true;
  }

  function commitRename() {
    editing = false;
    const trimmed = editName.trim();
    if (trimmed && trimmed !== session.name) {
      onrename(trimmed);
    }
  }

  const statusClasses: Record<Session["status"], string> = {
    idle: "bg-green shadow-[0_0_6px_var(--color-green-dim)]",
    thinking: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
    generating: "bg-blue shadow-[0_0_8px_var(--color-blue-dim)]",
    error: "bg-red shadow-[0_0_4px_var(--color-red-dim)]",
    disconnected: "bg-gray",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
  };

  const railClasses: Record<Session["status"], string> = {
    idle: "bg-accent shadow-[0_0_6px_var(--color-blue-dim)]",
    thinking: "bg-accent shadow-[0_0_6px_var(--color-blue-dim)]",
    generating: "bg-accent shadow-[0_0_6px_var(--color-blue-dim)]",
    error: "bg-red shadow-[0_0_6px_var(--color-red-dim)]",
    disconnected: "bg-gray",
    attention: "bg-amber shadow-[0_0_6px_var(--color-amber-dim)]",
  };

  const pulsingStatuses: Session["status"][] = ["thinking", "generating", "attention"];

  let projectName = $derived(
    session.projectId ? $projects.find((p) => p.id === session.projectId)?.name ?? null : null
  );

  let sessionWatches = $derived(
    $watchState.filter(
      (w) => w.scope.type === "session" && w.scope.sessionId === session.id
    )
  );

  let watchOutcomes = $derived(
    sessionWatches
      .map((w) => w.lastResult?.outcome ?? null)
      .filter((o): o is WatchOutcome => o !== null)
  );

  let isFlashing = $derived($flashingSessions.has(session.id));

  let unreadCount = $derived($unreadBySession.get(session.id) ?? 0);

  let flashColor = $derived.by(() => {
    if (!isFlashing) return "";
    const hasFailure = watchOutcomes.includes("failure");
    const hasSuccess = watchOutcomes.includes("success");
    if (hasFailure) return "var(--color-red-dim, rgba(239,68,68,0.15))";
    if (hasSuccess) return "var(--color-green-dim, rgba(34,197,94,0.15))";
    return "var(--color-amber-dim, rgba(245,158,11,0.15))";
  });
</script>

<!-- Use div, not button, to avoid invalid nested <button> for the close control -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative mb-1 w-full cursor-pointer overflow-hidden px-3 py-2 text-left transition-colors duration-150
    {active
      ? 'bg-bg-active shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
      : 'bg-transparent hover:bg-bg-active/40'}
    {isFlashing ? 'watch-flash' : ''}"
  style:--flash-color={flashColor}
  onclick={onselect}
  oncontextmenu={(e) => {
    if (oncontextmenu) {
      e.preventDefault();
      oncontextmenu(e);
    }
  }}
  title={session.worktreePath}
>
  {#if active || pulsingStatuses.includes(session.status) || session.status === "error"}
    <div
      class="absolute left-0 top-1.5 bottom-1.5 w-[2px] rounded-full {active ? 'bg-accent shadow-[0_0_6px_var(--color-blue-dim)]' : railClasses[session.status]}"
    ></div>
  {/if}

  <div class="mb-1 flex items-start gap-2">
    <div class="relative mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center">
      {#if pulsingStatuses.includes(session.status)}
        <span class="absolute inline-flex h-2 w-2 rounded-full {statusClasses[session.status]} animate-ping opacity-50"></span>
      {/if}
      <span class="relative inline-flex h-2 w-2 rounded-full {statusClasses[session.status]}"></span>
    </div>

    {#if editing}
      <input
        class="flex-1 border border-accent-dim/30 bg-bg-deep px-2 py-1.5 text-[13px] font-semibold tracking-tight text-text-primary outline-none"
        bind:value={editName}
        onblur={commitRename}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.stopPropagation();
            commitRename();
          }
          if (e.key === "Escape") {
            e.stopPropagation();
            editing = false;
          }
        }}
      />
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span
        class="flex-1 truncate text-[13px] font-semibold tracking-tight {active ? 'text-text-primary' : 'text-text-secondary'}"
        ondblclick={startEditing}
      >
        {session.name}
      </span>
    {/if}

    {#if unreadCount > 0}
      <span
        class="inline-flex h-4 min-w-[16px] shrink-0 items-center justify-center rounded-full bg-accent-dim/30 px-1 text-[9px] font-semibold text-accent"
        title="{unreadCount} unread notification{unreadCount === 1 ? '' : 's'}"
      >{unreadCount > 99 ? "99+" : unreadCount}</span>
    {/if}
    {#if session.status === "disconnected"}
      <button
        class="cursor-pointer border border-accent-dim/20 bg-accent-dim/15 px-2 py-1 text-[11px] font-semibold text-accent hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
        onclick={(e) => {
          e.stopPropagation();
          onreconnect();
        }}
      >
        reconnect
      </button>
    {/if}
    <button
      class="cursor-pointer flex h-5 w-5 items-center justify-center bg-transparent text-[11px] leading-none text-text-secondary opacity-80 transition-all duration-150 group-hover:opacity-100 hover:bg-bg-hover hover:text-red focus-visible:opacity-100 focus-visible:outline-none"
      onclick={(e) => {
        e.stopPropagation();
        onclose();
      }}
    >
      &times;
    </button>
  </div>

  <div class="flex items-center gap-1.5 pl-4">
    {#if session.isGitRepo}
      <span class="flex items-center gap-1 font-mono text-[11px] {active ? 'text-text-secondary' : 'text-text-muted'}">
        <span class="text-[10px] text-text-secondary">&#9095;</span>
        {session.branch}
      </span>
    {/if}
    <span class="truncate text-[10px] text-text-muted">{pathLabel(session.worktreePath)}</span>
    {#if projectName}
      <span class="bg-accent-dim/15 px-1.5 py-0.5 text-[10px] font-semibold text-accent">{projectName}</span>
    {/if}
    <span class="ml-auto text-[10px] font-semibold text-text-muted">
      {session.cost != null ? `$${session.cost.toFixed(2)}` : ""}
    </span>
    {#if watchOutcomes.length > 0}
      <div class="flex items-center gap-1">
        {#each watchOutcomes as outcome}
          <span
            class="inline-block h-1.5 w-1.5 rounded-full
              {outcome === 'success' ? 'bg-green' : outcome === 'failure' ? 'bg-red' : 'bg-amber'}"
            class:animate-pulse={outcome === "inProgress"}
          ></span>
        {/each}
      </div>
    {/if}
  </div>

  {#if session.permissionInfo}
    <div class="mt-1.5 border border-amber/10 bg-amber/10 px-2.5 py-1.5">
      <div class="flex items-start gap-1">
        <span
          class="block flex-1 truncate font-mono text-[10px] text-amber"
          title={JSON.stringify(session.permissionInfo.toolInput, null, 2)}
        >
          {formatPermission(session.permissionInfo)}
        </span>
        <button
          class="cursor-pointer shrink-0 bg-transparent text-[10px] leading-none text-amber/60 hover:text-amber"
          onclick={(e) => {
            e.stopPropagation();
            ondismiss();
          }}
        >&times;</button>
      </div>
      {#if session.status === "attention"}
        <div class="mt-1.5 flex gap-1">
          <button
            class="cursor-pointer bg-green/10 px-2.5 py-1 text-[11px] font-semibold text-green transition-colors hover:bg-green/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
            onclick={(e) => {
              e.stopPropagation();
              onapprove();
            }}
          >
            &#10003; Allow
          </button>
          <button
            class="cursor-pointer bg-accent-dim/15 px-2.5 py-1 text-[11px] font-semibold text-accent transition-colors hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
            onclick={(e) => {
              e.stopPropagation();
              onalways();
            }}
          >
            &#10003; Always
          </button>
          <button
            class="cursor-pointer bg-red/10 px-2.5 py-1 text-[11px] font-semibold text-red transition-colors hover:bg-red/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
            onclick={(e) => {
              e.stopPropagation();
              ondeny();
            }}
          >
            &#10007; Deny
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .watch-flash {
    animation: watch-flash-anim 1.5s ease-out;
  }

  @keyframes watch-flash-anim {
    0% { background-color: var(--flash-color); }
    100% { background-color: transparent; }
  }
</style>
