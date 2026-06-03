<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { settings, updateSetting } from "$lib/stores/settings";

  async function browseNotesVault() {
    const selected = await open({
      directory: true,
      title: "Select Notes Vault Location",
    });
    if (selected) updateSetting("notesVaultRoot", selected as string);
  }
</script>

<div class="py-2">
  <div class="flex items-center justify-between">
    <div>
      <div class="text-[13px]">Vault location</div>
      <div class="text-[11px] text-text-muted mt-0.5">
        Where Roux stores notes. Works as a standalone folder or a subdirectory
        inside an Obsidian vault.
      </div>
    </div>
  </div>
  <div class="mt-2 flex gap-1">
    <input
      class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none flex-1 focus:border-accent-dim"
      value={$settings.notesVaultRoot ?? ""}
      oninput={(e) =>
        updateSetting("notesVaultRoot", e.currentTarget.value || null)}
      placeholder="~/Documents/Roux"
    />
    <button
      class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
      onclick={browseNotesVault}>...</button
    >
  </div>
  <div class="mt-1.5 text-[11px] text-text-muted">
    Leave blank to use the default location. Changing this does not move
    existing notes.
  </div>
</div>
<div class="flex items-center justify-between py-2 mt-2">
  <div>
    <div class="text-[13px]">Include web anchors</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Add HTML anchor tags for compatibility with static site generators.
      Disable for cleaner markdown in Obsidian.
    </div>
  </div>
  <button
    aria-label="Toggle web anchors in notes"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {($settings.notesIncludeWebAnchors ?? true)
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() =>
      updateSetting(
        "notesIncludeWebAnchors",
        !($settings.notesIncludeWebAnchors ?? true),
      )}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {($settings.notesIncludeWebAnchors ?? true)
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>
