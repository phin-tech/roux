<script lang="ts">
  import {
    mainViewRoute,
    closeMainView,
    openMainView,
  } from "$lib/stores/mainView";
  import { sessionDisplayName, sessionList } from "$lib/stores/sessions";
  import {
    closeExternalToolRun,
    externalToolRuns,
    restartExternalToolRun,
    setExternalToolLogsOpen,
  } from "$lib/stores/externalTools";
  import { focusExternalToolSettings } from "$lib/stores/settingsFocus";
  import BoardMainView from "./BoardMainView.svelte";
  import ExternalToolMainView from "./ExternalToolMainView.svelte";
  import MainViewShell from "./MainViewShell.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";
  import SessionDetailView from "./SessionDetailView.svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import ScrollText from "@lucide/svelte/icons/scroll-text";

  let route = $derived($mainViewRoute);
  let session = $derived(
    route?.kind === "sessionDetail"
      ? ($sessionList.find((s) => s.id === route.sessionId) ?? null)
      : null,
  );
  let externalToolRun = $derived(
    route?.kind === "externalTool"
      ? ($externalToolRuns.get(route.runId) ?? null)
      : null,
  );
  let title = $derived.by(() => {
    if (!route) return "";
    switch (route.kind) {
      case "board":
        return "Board";
      case "sessionDetail":
        return session ? sessionDisplayName(session) : "Session Details";
      case "externalTool":
        return externalToolRun?.toolName ?? "External Tool";
      case "preferences":
        return "Preferences";
    }
  });
  let subtitle = $derived.by(() => {
    if (!route) return null;
    switch (route.kind) {
      case "board":
        return null;
      case "sessionDetail":
        if (!session) return "Session no longer available";
        return [session.status, session.branch, session.worktreePath]
          .filter((part) => part && part.trim())
          .join(" · ");
      case "externalTool":
        if (!externalToolRun) return "Run no longer available";
        return [
          externalToolRun.surface,
          externalToolRun.sessionId
            ? `session ${externalToolRun.sessionId.slice(0, 8)}`
            : "global",
          externalToolRun.status,
        ].join(" · ");
      case "preferences":
        return null;
    }
  });
  let closeLabel = $derived.by(() => {
    if (!route) return "Close View";
    switch (route.kind) {
      case "board":
        return "Close Board";
      case "sessionDetail":
        return "Close Session Details";
      case "externalTool":
        return `Close ${externalToolRun?.toolName ?? "External Tool"}`;
      case "preferences":
        return "Close Preferences";
    }
  });

  function closeRoute(): void {
    if (route?.kind === "externalTool") {
      void closeExternalToolRun(route.runId);
      return;
    }
    closeMainView();
  }

  function editExternalTool(): void {
    if (!externalToolRun) return;
    focusExternalToolSettings(externalToolRun.toolId);
    openMainView({
      kind: "preferences",
      category: "externalTools",
      externalToolId: externalToolRun.toolId,
    });
  }
</script>

{#if route}
  {#snippet actions()}
    {#if route?.kind === "externalTool" && externalToolRun}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={() => void restartExternalToolRun(externalToolRun!.id)}
        aria-label={`Restart ${externalToolRun.toolName}`}
        title="Restart"
      >
        <RefreshCw size={13} />
      </button>
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
        onclick={editExternalTool}
        aria-label={`Edit ${externalToolRun.toolName}`}
        title="Edit"
      >
        <SettingsIcon size={13} />
      </button>
      {#if externalToolRun.surface === "web"}
        <button
          type="button"
          class="flex h-6 w-6 items-center justify-center rounded text-text-muted transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50 {externalToolRun.logsOpen
            ? 'bg-bg-hover text-text-primary'
            : ''}"
          onclick={() =>
            setExternalToolLogsOpen(
              externalToolRun!.id,
              !externalToolRun!.logsOpen,
            )}
          aria-label={`${externalToolRun.logsOpen ? "Hide" : "Show"} ${externalToolRun.toolName} logs`}
          title="Logs"
        >
          <ScrollText size={13} />
        </button>
      {/if}
    {/if}
  {/snippet}

  <MainViewShell {title} {subtitle} {closeLabel} onclose={closeRoute} {actions}>
    {#if route.kind === "board"}
      <BoardMainView />
    {:else if route.kind === "sessionDetail"}
      <SessionDetailView sessionId={route.sessionId} />
    {:else if route.kind === "externalTool"}
      <ExternalToolMainView runId={route.runId} />
    {:else if route.kind === "preferences"}
      <SettingsPanel
        visible={true}
        onclose={closeMainView}
        initialCategory={route.category ?? "general"}
        externalToolId={route.externalToolId ?? null}
      />
    {/if}
  </MainViewShell>
{/if}
