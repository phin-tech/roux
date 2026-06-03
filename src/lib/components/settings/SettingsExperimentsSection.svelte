<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";
  import {
    EXPERIMENTS,
    currentExperimentValue,
    withExperimentValue,
  } from "$lib/experiments";
</script>

<p class="text-[11px] text-text-muted mb-3">
  Toggle in-progress features. Experiments default to off and may change
  behavior, persistence, or performance. Disable if you hit issues.
</p>
{#each EXPERIMENTS as exp (exp.id)}
  <div class="flex items-start justify-between gap-3 py-2">
    <div>
      <div class="text-[13px]">{exp.label}</div>
      <div class="text-[11px] text-text-muted mt-0.5">{exp.description}</div>
    </div>
    {#if exp.kind === "boolean"}
      {@const current = currentExperimentValue(
        $settings.experiments,
        exp.id,
      ) as boolean}
      <button
        aria-label="Toggle {exp.label}"
        class="w-9 h-5 rounded-full relative cursor-pointer transition-all border shrink-0
          {current
          ? 'bg-accent-dim border-accent'
          : 'bg-bg-deep border-border'}"
        onclick={() =>
          updateSetting(
            "experiments",
            withExperimentValue($settings.experiments, exp.id, !current),
          )}
      >
        <div
          class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {current ? 'left-[18px] bg-accent' : 'left-0.5 bg-text-secondary'}"
        ></div>
      </button>
    {:else}
      {@const current = currentExperimentValue(
        $settings.experiments,
        exp.id,
      ) as string}
      <select
        aria-label="Select {exp.label}"
        class="bg-bg-deep border border-border rounded px-2 py-1 text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6 shrink-0"
        value={current}
        onchange={(e) =>
          updateSetting(
            "experiments",
            withExperimentValue(
              $settings.experiments,
              exp.id,
              e.currentTarget.value,
            ),
          )}
      >
        {#each exp.options as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
    {/if}
  </div>
{/each}
