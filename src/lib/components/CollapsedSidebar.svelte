<script lang="ts">
  import SessionDot from "./SessionDot.svelte";
  import { sessionState, setActiveSession } from "$lib/stores/sessions";
  import { projects } from "$lib/stores/projects";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { getGroupedSessions } from "$lib/sessions/order";

  let grouped = $derived(
    getGroupedSessions(
      $sessionState.sessions,
      $projects,
      $settings.groupBy ?? "repo",
    ),
  );

  function expand() {
    updateSetting("sidebarCollapsed", false);
  }
</script>

<div
  class="flex h-full w-[44px] shrink-0 flex-col overflow-hidden bg-bg-base/96 shadow-[0_0_0_1px_rgba(255,255,255,0.03)]"
>
  <div class="flex h-9 shrink-0 items-center justify-center">
    <button
      type="button"
      class="flex h-6 w-6 cursor-pointer items-center justify-center text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
      onclick={expand}
      title="Expand sidebar"
      aria-label="Expand sidebar"
    >
      <span class="text-xs">{$settings.tabPosition === "right" ? "\u25C0" : "\u25B6"}</span>
    </button>
  </div>

  <div class="app-scrollbar flex flex-1 flex-col gap-1 overflow-y-auto overflow-x-hidden px-2 pt-1 pb-2">
    {#each grouped as group, i (group.key)}
      {#if i > 0}
        <div class="my-1 h-px w-full bg-white/6"></div>
      {/if}
      {#each group.sessions as session (session.id)}
        <SessionDot
          {session}
          active={session.id === $sessionState.activeSessionId}
          onselect={() => setActiveSession(session.id)}
        />
      {/each}
    {/each}
  </div>
</div>
