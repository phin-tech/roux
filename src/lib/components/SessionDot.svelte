<script lang="ts">
  import type { Session } from "$lib/types";
  import { sessionDisplayName } from "$lib/stores/sessions";

  interface Props {
    session: Session;
    active: boolean;
    onselect: () => void;
  }

  let { session, active, onselect }: Props = $props();

  let displayName = $derived(sessionDisplayName(session));
  let initial = $derived((displayName.trim()[0] ?? "?").toUpperCase());
</script>

<button
  type="button"
  class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md border text-[11px] font-semibold transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50
    {active
      ? 'border-accent bg-accent/20 text-accent ring-1 ring-accent/40'
      : 'border-border-subtle bg-bg-surface/70 text-text-secondary hover:border-accent-dim/40 hover:bg-bg-hover hover:text-text-primary'}"
  title={displayName}
  aria-label={displayName}
  aria-pressed={active}
  onclick={onselect}
>
  {initial}
</button>
