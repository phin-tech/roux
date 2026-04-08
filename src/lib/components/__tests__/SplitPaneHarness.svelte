<script lang="ts">
  import SplitPane from "../SplitPane.svelte";
  import { sessionLayouts } from "$lib/panes/layout";

  interface Props {
    sessionId: string;
  }

  let { sessionId }: Props = $props();
  const tree = $derived($sessionLayouts.get(sessionId));
</script>

{#if tree}
  <SplitPane node={tree} {sessionId} />
{/if}
