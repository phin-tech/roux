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
    idle: "bg-green shadow-[0_0_12px_var(--color-green-dim)]",
    thinking: "bg-amber shadow-[0_0_14px_var(--color-amber-dim)]",
    generating: "bg-blue shadow-[0_0_14px_var(--color-blue-dim)]",
    error: "bg-red shadow-[0_0_6px_var(--color-red-dim)]",
    disconnected: "bg-gray opacity-60",
    attention: "bg-amber shadow-[0_0_14px_var(--color-amber-dim)]",
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
    idle: "bg-sky-400 shadow-[0_0_10px_rgba(56,189,248,0.25)]",
    thinking: "bg-sky-400 shadow-[0_0_12px_rgba(56,189,248,0.45)]",
    generating: "bg-sky-400 shadow-[0_0_12px_rgba(56,189,248,0.45)]",
    error: "bg-rose-400 shadow-[0_0_12px_rgba(251,113,133,0.4)]",
    disconnected: "bg-zinc-600",
    attention: "bg-amber-400 shadow-[0_0_12px_rgba(251,191,36,0.38)]",
  };

  const pulsingStatuses: Session["status"][] = ["thinking", "generating", "attention"];
</script>

<!-- Use div, not button, to avoid invalid nested <button> for the close control -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative mb-2 w-full cursor-pointer overflow-hidden rounded-2xl px-3 py-3 text-left transition-colors duration-150
    {active
      ? 'bg-white/[0.05] shadow-[inset_0_1px_0_rgba(255,255,255,0.04),0_16px_32px_rgba(0,0,0,0.22)]'
      : 'bg-transparent hover:bg-white/[0.03]'}"
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
      class="absolute left-0 top-2 bottom-2 w-[2px] rounded-full {active ? 'bg-sky-400 shadow-[0_0_12px_rgba(56,189,248,0.45)]' : railClasses[session.status]}"
    ></div>
  {/if}

  <div class="mb-2 flex items-start gap-2">
    <div class="relative mt-0.5 flex h-3 w-3 shrink-0 items-center justify-center">
      {#if pulsingStatuses.includes(session.status)}
        <span class="absolute inline-flex h-full w-full rounded-full {statusClasses[session.status]} animate-ping opacity-60"></span>
      {/if}
      <span class="relative inline-flex h-3 w-3 rounded-full {statusClasses[session.status]}"></span>
    </div>

    {#if editing}
      <input
        class="flex-1 rounded-lg border border-sky-400/30 bg-black/35 px-2 py-1 text-[13px] font-medium tracking-tight text-zinc-100 outline-none"
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
        class="flex-1 truncate text-[13px] font-semibold tracking-tight text-zinc-100"
        ondblclick={startEditing}
      >
        {session.name}
      </span>
    {/if}

    <span class="rounded-full px-2 py-0.5 text-[9px] font-medium uppercase tracking-[0.22em] {labelClasses[session.status]}">
      {labelText[session.status]}
    </span>
    {#if session.status === "disconnected"}
      <button
        class="cursor-pointer rounded-full border border-sky-400/20 bg-sky-500/10 px-2 py-0.5 text-[10px] font-medium text-sky-200 hover:bg-sky-500/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
        onclick={(e) => {
          e.stopPropagation();
          onreconnect();
        }}
      >
        reconnect
      </button>
    {/if}
    <button
      class="cursor-pointer rounded-lg border border-transparent bg-transparent p-1 text-sm text-zinc-600 opacity-0 transition-all duration-150 group-hover:opacity-100 hover:bg-white/[0.05] hover:text-rose-300 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
      onclick={(e) => {
        e.stopPropagation();
        onclose();
      }}
    >
      &times;
    </button>
  </div>

  <div class="flex items-center gap-2 pl-5">
    <span class="flex items-center gap-1 font-mono text-[11px] text-zinc-300">
      <span class="text-[10px] opacity-70">&#9095;</span>
      {session.branch}
    </span>
    <span class="truncate text-[10px] text-zinc-600">{pathLabel(session.worktreePath)}</span>
    <span class="ml-auto text-[10px] font-medium text-zinc-600">
      {session.cost != null ? `$${session.cost.toFixed(2)}` : ""}
    </span>
  </div>

  {#if session.permissionInfo}
    <div class="mt-2 rounded-xl border border-amber/10 bg-amber/10 px-3 py-2">
      <span
        class="block truncate font-mono text-[11px] text-amber"
        title={JSON.stringify(session.permissionInfo.toolInput, null, 2)}
      >
        {formatPermission(session.permissionInfo)}
      </span>
      {#if session.status === "attention"}
        <div class="mt-2 flex gap-1.5">
          <button
            class="cursor-pointer rounded-full bg-green/10 px-2.5 py-1 text-[10px] font-medium text-green transition-colors hover:bg-green/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
            onclick={(e) => {
              e.stopPropagation();
              onapprove();
            }}
          >
            &#10003; Allow
          </button>
          <button
            class="cursor-pointer rounded-full bg-sky-500/10 px-2.5 py-1 text-[10px] font-medium text-sky-200 transition-colors hover:bg-sky-500/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
            onclick={(e) => {
              e.stopPropagation();
              onalways();
            }}
          >
            &#10003; Always
          </button>
          <button
            class="cursor-pointer rounded-full bg-red/10 px-2.5 py-1 text-[10px] font-medium text-red transition-colors hover:bg-red/20 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-zinc-950"
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
