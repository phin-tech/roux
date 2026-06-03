<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";
  import { notificationsPush } from "$lib/tauri";

  let notifTestStatus = $state<"idle" | "sent" | "error">("idle");
  let notifTestError = $state<string | null>(null);

  async function sendTestNotification() {
    notifTestStatus = "idle";
    notifTestError = null;
    try {
      await notificationsPush({
        level: "attention",
        source: { type: "cli" },
        title: "Roux notification test",
        subtitle: null,
        body: "If you saw a macOS notification, permissions are set up correctly.",
        sessionId: null,
        actions: [],
        dedupKey: null,
      });
      notifTestStatus = "sent";
    } catch (e) {
      notifTestStatus = "error";
      notifTestError = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Enable OS notifications</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Master switch for macOS notification fan-out. The in-app pane always
      works.
    </div>
  </div>
  <button
    aria-label="Toggle OS notifications"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {$settings.notificationsEnabled
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() =>
      updateSetting("notificationsEnabled", !$settings.notificationsEnabled)}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {$settings.notificationsEnabled
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>

<div class="flex items-center justify-between py-2">
  <div>
    <div class="text-[13px]">Agent completion notifications</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      Notify when an agent finishes in a pane other than the one you're focused
      on. Errors notify regardless.
    </div>
  </div>
  <button
    aria-label="Toggle agent completion notifications"
    class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
      {($settings.agentCompletionNotificationsEnabled ?? true)
      ? 'bg-accent-dim border-accent'
      : 'bg-bg-deep border-border'}"
    onclick={() =>
      updateSetting(
        "agentCompletionNotificationsEnabled",
        !($settings.agentCompletionNotificationsEnabled ?? true),
      )}
  >
    <div
      class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
      {($settings.agentCompletionNotificationsEnabled ?? true)
        ? 'left-[18px] bg-accent'
        : 'left-0.5 bg-text-secondary'}"
    ></div>
  </button>
</div>

<div
  class="mt-3 rounded-lg border border-amber/20 bg-amber/5 p-3 text-[11px] text-text-secondary"
>
  <div class="mb-1 font-semibold text-amber">macOS quirk</div>
  <div class="leading-relaxed">
    In dev mode Roux borrows <span class="font-mono text-[10px]"
      >com.apple.Terminal</span
    >'s notification identity (unsigned binaries can't own a bundle id). If you
    don't see the test notification below, open
    <span class="text-text-primary"
      >System Settings → Notifications → Terminal</span
    >
    and make sure "Allow Notifications" is on. Bundled release builds use
    <span class="font-mono text-[10px]">com.phin-tech.roux</span>.
  </div>
</div>

<div class="mt-3 flex items-center justify-between gap-3">
  <div>
    <div class="text-[13px]">Test notification</div>
    <div class="text-[11px] text-text-muted mt-0.5">
      {#if notifTestStatus === "sent"}
        Test fired — check macOS notification center. If nothing shows, fix
        permissions above.
      {:else if notifTestStatus === "error"}
        <span class="text-red">Failed: {notifTestError}</span>
      {:else}
        Pushes an Attention-level notification through the service
      {/if}
    </div>
  </div>
  <button
    class="shrink-0 cursor-pointer rounded-lg border border-border-subtle bg-bg-deep px-3 py-1.5 text-[12px] font-semibold text-text-primary hover:border-accent hover:bg-bg-hover"
    onclick={sendTestNotification}
  >
    Send test
  </button>
</div>
