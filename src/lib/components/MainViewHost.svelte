<script lang="ts">
  import { mainViewRoute, closeMainView } from "$lib/stores/mainView";
  import { sessionDisplayName, sessionList } from "$lib/stores/sessions";
  import BoardMainView from "./BoardMainView.svelte";
  import MainViewShell from "./MainViewShell.svelte";
  import SessionDetailView from "./SessionDetailView.svelte";

  let route = $derived($mainViewRoute);
  let session = $derived(
    route?.kind === "sessionDetail"
      ? ($sessionList.find((s) => s.id === route.sessionId) ?? null)
      : null,
  );
  let title = $derived.by(() => {
    if (!route) return "";
    switch (route.kind) {
      case "board":
        return "Board";
      case "sessionDetail":
        return session ? sessionDisplayName(session) : "Session Details";
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
    }
  });
  let closeLabel = $derived.by(() => {
    if (!route) return "Close View";
    switch (route.kind) {
      case "board":
        return "Close Board";
      case "sessionDetail":
        return "Close Session Details";
    }
  });
</script>

{#if route}
  <MainViewShell {title} {subtitle} {closeLabel} onclose={closeMainView}>
    {#if route.kind === "board"}
      <BoardMainView />
    {:else if route.kind === "sessionDetail"}
      <SessionDetailView sessionId={route.sessionId} />
    {/if}
  </MainViewShell>
{/if}
