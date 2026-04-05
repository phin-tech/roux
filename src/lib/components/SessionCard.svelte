<script lang="ts">
  import type { Session, PermissionInfo } from "$lib/types";

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

  let { session, active, onselect, onclose, onrename, onreconnect, onapprove, onalways, ondeny, oncontextmenu }: Props = $props();

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

  let editing = $state(false);
  let editName = $state("");

  $effect(() => {
    if (!editing) {
      editName = session.name;
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
    idle: "border border-green/15 text-green bg-green/10",
    thinking: "border border-amber/15 text-amber bg-amber/10",
    generating: "border border-blue/15 text-blue bg-blue/10",
    error: "border border-red/15 text-red bg-red/10",
    disconnected: "border border-gray/15 text-gray bg-gray/15",
    attention: "border border-amber/15 text-amber bg-amber/15",
  };

  const labelText: Record<Session["status"], string> = {
    idle: "idle",
    thinking: "think",
    generating: "gen",
    error: "error",
    disconnected: "disc",
    attention: "wait",
  };

  const pulsingStatuses: Session["status"][] = ["thinking", "generating", "attention"];
</script>

<!-- Use div, not button, to avoid invalid nested <button> for the close control -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative mb-2 w-full cursor-pointer rounded-2xl border p-3 text-left transition-all duration-150
    {active
      ? 'border-sky-400/30 bg-bg-active shadow-[0_18px_40px_rgba(2,6,23,0.32),inset_0_1px_0_rgba(255,255,255,0.04)]'
      : 'border-transparent bg-white/[0.02] hover:border-white/8 hover:bg-bg-hover/70 hover:shadow-[0_12px_28px_rgba(2,6,23,0.22)]'}"
  onclick={onselect}
  oncontextmenu={(e) => { if (oncontextmenu) { e.preventDefault(); oncontextmenu(e); } }}
  title={session.worktreePath}
>
  {#if active}
    <div class="absolute inset-x-4 bottom-0 h-px bg-gradient-to-r from-transparent via-sky-300/80 to-transparent"></div>
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
        class="flex-1 rounded-lg border border-sky-400/30 bg-black/35 px-2 py-1 text-[13px] font-medium tracking-tight text-text-primary outline-none"
        bind:value={editName}
        onblur={commitRename}
        onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); commitRename(); } if (e.key === 'Escape') { e.stopPropagation(); editing = false; } }}
      />
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span
        class="flex-1 truncate text-[13px] font-semibold tracking-tight text-text-primary"
        ondblclick={startEditing}
      >
        {session.name}
      </span>
    {/if}

    <span class="rounded-full px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.18em] {labelClasses[session.status]}">
      {labelText[session.status]}
    </span>
    {#if session.status === "disconnected"}
      <button
        class="rounded-full border border-sky-400/20 bg-sky-500/10 px-2 py-0.5 text-[10px] font-medium text-sky-200 cursor-pointer hover:bg-sky-500/20"
        onclick={(e) => { e.stopPropagation(); onreconnect(); }}
      >
        reconnect
      </button>
    {/if}
    <button
      class="rounded-lg border border-transparent bg-transparent p-1 text-sm text-text-muted opacity-0 transition-all duration-150 cursor-pointer group-hover:opacity-100 hover:border-white/8 hover:bg-bg-elevated hover:text-red"
      onclick={(e) => { e.stopPropagation(); onclose(); }}
    >
      &times;
    </button>
  </div>

  <div class="flex items-center gap-2 pl-5">
    <span class="flex items-center gap-1 font-mono text-[11px] text-sky-200">
      <span class="text-[10px] opacity-70">&#9095;</span>
      {session.branch}
    </span>
    <span class="ml-auto text-[10px] font-medium text-text-muted">
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
            class="rounded-full bg-green/10 px-2.5 py-1 text-[10px] font-medium text-green cursor-pointer hover:bg-green/20 transition-colors"
            onclick={(e) => { e.stopPropagation(); onapprove(); }}
          >
            &#10003; Allow
          </button>
          <button
            class="rounded-full bg-sky-500/10 px-2.5 py-1 text-[10px] font-medium text-sky-200 cursor-pointer hover:bg-sky-500/20 transition-colors"
            onclick={(e) => { e.stopPropagation(); onalways(); }}
          >
            &#10003; Always
          </button>
          <button
            class="rounded-full bg-red/10 px-2.5 py-1 text-[10px] font-medium text-red cursor-pointer hover:bg-red/20 transition-colors"
            onclick={(e) => { e.stopPropagation(); ondeny(); }}
          >
            &#10007; Deny
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  @keyframes stream {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
</style>
