<script lang="ts">
  import type { Session, PermissionInfo } from "$lib/types";
  import { renameSignal } from "$lib/stores/sessions";

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
    disconnected: "bg-gray opacity-40",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)]",
  };

  const labelClasses: Record<Session["status"], string> = {
    idle: "border border-green/15 bg-green/10 text-green",
    thinking: "border border-amber/15 bg-amber/10 text-amber",
    generating: "border border-blue/15 bg-blue/10 text-blue",
    error: "border border-red/15 bg-red/10 text-red",
    disconnected: "border border-gray/15 bg-gray/15 text-gray",
    attention: "border border-amber/15 bg-amber/15 text-amber",
  };

  const labelText: Record<Session["status"], string> = {
    idle: "idle",
    thinking: "think",
    generating: "gen",
    error: "error",
    disconnected: "disc",
    attention: "wait",
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
</script>

<!-- Use div, not button, to avoid invalid nested <button> for the close control -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative mb-1 w-full cursor-pointer overflow-hidden rounded-lg px-3 py-2 text-left transition-colors duration-150
    {active
      ? 'bg-white/[0.05] shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
      : 'bg-transparent hover:bg-white/[0.02]'}"
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
        class="flex-1 rounded-md border border-accent-dim/30 bg-bg-deep px-2 py-1 text-[12px] font-medium tracking-tight text-text-primary outline-none"
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
        class="flex-1 truncate text-[12px] font-medium tracking-tight {active ? 'text-text-primary' : 'text-text-secondary'}"
        ondblclick={startEditing}
      >
        {session.name}
      </span>
    {/if}

    <span class="rounded-full px-1.5 py-0.5 text-[8px] font-medium uppercase tracking-[0.2em] {labelClasses[session.status]}">
      {labelText[session.status]}
    </span>
    {#if session.status === "disconnected"}
      <button
        class="cursor-pointer rounded-full border border-accent-dim/20 bg-accent-dim/15 px-1.5 py-0.5 text-[9px] font-medium text-accent hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
        onclick={(e) => {
          e.stopPropagation();
          onreconnect();
        }}
      >
        reconnect
      </button>
    {/if}
    <button
      class="cursor-pointer rounded-md border border-transparent bg-transparent p-0.5 text-sm text-text-muted opacity-0 transition-all duration-150 group-hover:opacity-100 hover:bg-white/[0.05] hover:text-red focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
      onclick={(e) => {
        e.stopPropagation();
        onclose();
      }}
    >
      &times;
    </button>
  </div>

  <div class="flex items-center gap-1.5 pl-4">
    <span class="flex items-center gap-1 font-mono text-[10px] {active ? 'text-text-secondary' : 'text-text-muted'}">
      <span class="text-[9px] opacity-60">&#9095;</span>
      {session.branch}
    </span>
    <span class="truncate text-[9px] text-text-muted">{pathLabel(session.worktreePath)}</span>
    <span class="ml-auto text-[9px] font-medium text-text-muted">
      {session.cost != null ? `$${session.cost.toFixed(2)}` : ""}
    </span>
  </div>

  {#if session.permissionInfo}
    <div class="mt-1.5 rounded-lg border border-amber/10 bg-amber/10 px-2.5 py-1.5">
      <span
        class="block truncate font-mono text-[10px] text-amber"
        title={JSON.stringify(session.permissionInfo.toolInput, null, 2)}
      >
        {formatPermission(session.permissionInfo)}
      </span>
      {#if session.status === "attention"}
        <div class="mt-1.5 flex gap-1">
          <button
            class="cursor-pointer rounded-full bg-green/10 px-2 py-0.5 text-[9px] font-medium text-green transition-colors hover:bg-green/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
            onclick={(e) => {
              e.stopPropagation();
              onapprove();
            }}
          >
            &#10003; Allow
          </button>
          <button
            class="cursor-pointer rounded-full bg-accent-dim/15 px-2 py-0.5 text-[9px] font-medium text-accent transition-colors hover:bg-accent-dim/24 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
            onclick={(e) => {
              e.stopPropagation();
              onalways();
            }}
          >
            &#10003; Always
          </button>
          <button
            class="cursor-pointer rounded-full bg-red/10 px-2 py-0.5 text-[9px] font-medium text-red transition-colors hover:bg-red/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 focus-visible:ring-offset-1 focus-visible:ring-offset-bg-deep"
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
