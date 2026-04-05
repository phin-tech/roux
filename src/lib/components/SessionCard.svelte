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
  }

  let { session, active, onselect, onclose, onrename, onreconnect, onapprove, onalways, ondeny }: Props = $props();

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
  let editName = $state(session.name);

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
    thinking: "bg-amber shadow-[0_0_6px_var(--color-amber-dim)] animate-pulse",
    generating: "bg-blue shadow-[0_0_6px_var(--color-blue-dim)] animate-[stream_1.5s_ease-in-out_infinite]",
    error: "bg-red shadow-[0_0_6px_var(--color-red-dim)]",
    disconnected: "bg-gray opacity-60",
    attention: "bg-amber shadow-[0_0_8px_var(--color-amber-dim)] animate-pulse",
  };

  const labelClasses: Record<Session["status"], string> = {
    idle: "text-green bg-green/10",
    thinking: "text-amber bg-amber/10",
    generating: "text-blue bg-blue/10",
    error: "text-red bg-red/10",
    disconnected: "text-gray bg-gray/15",
    attention: "text-amber bg-amber/15",
  };

  const labelText: Record<Session["status"], string> = {
    idle: "idle",
    thinking: "think",
    generating: "gen",
    error: "error",
    disconnected: "disc",
    attention: "wait",
  };
</script>

<!-- Use div, not button, to avoid invalid nested <button> for the close control -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="w-full text-left p-2.5 rounded-lg cursor-pointer transition-all duration-150 relative border group
    {active
      ? 'bg-bg-active border-border'
      : 'border-transparent hover:bg-bg-hover'}"
  onclick={onselect}
  title={session.worktreePath}
>
  {#if active}
    <div class="absolute left-0 top-2 bottom-2 w-0.5 bg-accent rounded-r"></div>
  {/if}

  <div class="flex items-center gap-2 mb-1">
    <div class="w-2 h-2 rounded-full shrink-0 {statusClasses[session.status]}"></div>

    {#if editing}
      <input
        class="text-[13px] font-medium text-text-primary bg-bg-deep border border-accent-dim rounded px-1 py-0 flex-1 outline-none font-sans"
        bind:value={editName}
        onblur={commitRename}
        onkeydown={(e) => { if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') { editing = false; } }}
      />
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span
        class="text-[13px] font-medium text-text-primary truncate flex-1"
        ondblclick={startEditing}
      >
        {session.name}
      </span>
    {/if}

    <span class="text-[10px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded {labelClasses[session.status]}">
      {labelText[session.status]}
    </span>
    {#if session.status === "disconnected"}
      <button
        class="text-[10px] font-medium text-accent bg-accent/10 px-1.5 py-0.5 rounded cursor-pointer border-none hover:bg-accent/20"
        onclick={(e) => { e.stopPropagation(); onreconnect(); }}
      >
        reconnect
      </button>
    {/if}
    <button
      class="opacity-0 group-hover:opacity-100 bg-transparent border-none text-text-muted hover:text-red hover:bg-bg-elevated text-sm p-0.5 rounded cursor-pointer transition-all duration-150"
      onclick={(e) => { e.stopPropagation(); onclose(); }}
    >
      &times;
    </button>
  </div>

  <div class="flex items-center gap-2 pl-4">
    <span class="font-mono text-[11px] text-accent flex items-center gap-1">
      <span class="text-[10px] opacity-70">&#9095;</span>
      {session.branch}
    </span>
    <span class="font-mono text-[10px] text-text-secondary ml-auto">
      {session.cost != null ? `$${session.cost.toFixed(2)}` : ""}
    </span>
  </div>

  {#if session.permissionInfo}
    <div class="pl-4 mt-1.5">
      <span
        class="block text-[11px] text-amber truncate font-mono"
        title={JSON.stringify(session.permissionInfo.toolInput, null, 2)}
      >
        {formatPermission(session.permissionInfo)}
      </span>
      {#if session.status === "attention"}
        <div class="flex gap-1.5 mt-1">
          <button
            class="text-[10px] font-medium text-green bg-green/10 px-2 py-0.5 rounded cursor-pointer border-none hover:bg-green/25 transition-colors"
            onclick={(e) => { e.stopPropagation(); onapprove(); }}
          >
            &#10003; Allow
          </button>
          <button
            class="text-[10px] font-medium text-accent bg-accent/10 px-2 py-0.5 rounded cursor-pointer border-none hover:bg-accent/25 transition-colors"
            onclick={(e) => { e.stopPropagation(); onalways(); }}
          >
            &#10003; Always
          </button>
          <button
            class="text-[10px] font-medium text-red bg-red/10 px-2 py-0.5 rounded cursor-pointer border-none hover:bg-red/25 transition-colors"
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
