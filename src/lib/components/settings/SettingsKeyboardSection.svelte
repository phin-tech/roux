<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";
</script>

<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Show pane hint overlay when holding Option</div>
    <div class="text-[11px] text-text-muted mt-0.5">Reveals pane numbers while Option is held. Option+digit shortcuts still work either way.</div>
  </div>
  <button
    aria-label="Toggle pane hint overlay on Option"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {$settings.showPaneHintsOnOption ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
    onclick={() => updateSetting("showPaneHintsOnOption", !$settings.showPaneHintsOnOption)}
  >
    <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {$settings.showPaneHintsOnOption ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
  </button>
</div>
<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Show session hint overlay when holding Command</div>
    <div class="text-[11px] text-text-muted mt-0.5">Reveals session shortcuts while Command is held. Command chord shortcuts still work either way.</div>
  </div>
  <button
    aria-label="Toggle session hint overlay on Command"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {$settings.showSessionHintsOnCommand !== false ? 'bg-accent-dim border-accent' : 'bg-bg-deep border-border'}"
    onclick={() => updateSetting("showSessionHintsOnCommand", !($settings.showSessionHintsOnCommand !== false))}
  >
    <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {$settings.showSessionHintsOnCommand !== false ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"></div>
  </button>
</div>
