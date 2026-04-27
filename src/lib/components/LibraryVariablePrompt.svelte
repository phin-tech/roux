<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import {
    cancelLibraryVariablePrompt,
    libraryVariablePrompt,
    setLibraryVariableValue,
    submitLibraryVariableForm,
  } from "$lib/stores/libraryVariablePrompt";

  const PANEL_WIDTH = 560;
  const MIN_VISIBLE = 80;

  let firstInputEl: HTMLInputElement | HTMLSelectElement | undefined = $state();
  let panelEl: HTMLElement | undefined = $state();
  let wasOpen = $state(false);
  let position = $state<{ x: number; y: number } | null>(null);
  let dragging = $state(false);
  let dragOffset = { x: 0, y: 0 };

  function firstInput(node: HTMLInputElement | HTMLSelectElement, enabled: boolean) {
    if (enabled) firstInputEl = node;
    return {
      update(nextEnabled: boolean) {
        if (nextEnabled) firstInputEl = node;
      },
      destroy() {
        if (firstInputEl === node) firstInputEl = undefined;
      },
    };
  }

  function submit() {
    submitLibraryVariableForm();
  }

  function defaultPosition(): { x: number; y: number } {
    const x = Math.max(16, Math.round((window.innerWidth - PANEL_WIDTH) / 2));
    const y = Math.max(16, Math.round(window.innerHeight * 0.18));
    return clampPosition({ x, y });
  }

  function clampPosition(p: { x: number; y: number }): { x: number; y: number } {
    const maxX = window.innerWidth - MIN_VISIBLE;
    const maxY = window.innerHeight - MIN_VISIBLE;
    const minX = MIN_VISIBLE - PANEL_WIDTH;
    const minY = 0;
    return {
      x: Math.min(maxX, Math.max(minX, p.x)),
      y: Math.min(maxY, Math.max(minY, p.y)),
    };
  }

  function onWindowResize(): void {
    if (position) position = clampPosition(position);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!$libraryVariablePrompt.open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      cancelLibraryVariablePrompt();
      return;
    }
    if (e.key === "Enter") {
      if (e.metaKey || e.ctrlKey) {
        e.preventDefault();
        submit();
      }
      return;
    }
    if (e.key === "Tab") {
      trapFocus(e);
    }
  }

  function focusableElements(): HTMLElement[] {
    if (!panelEl) return [];
    return Array.from(
      panelEl.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1'])",
      ),
    ).filter((el) => !el.hasAttribute("disabled") && el.offsetParent !== null);
  }

  function trapFocus(e: KeyboardEvent) {
    const focusable = focusableElements();
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function isInteractiveTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return target.closest("button, input, select, textarea, [role='button']") !== null;
  }

  function onHeaderPointerDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    if (isInteractiveTarget(e.target)) return;
    if (!position) position = defaultPosition();
    dragOffset = { x: e.clientX - position.x, y: e.clientY - position.y };
    dragging = true;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onHeaderPointerMove(e: PointerEvent): void {
    if (!dragging) return;
    position = clampPosition({
      x: e.clientX - dragOffset.x,
      y: e.clientY - dragOffset.y,
    });
  }

  function onHeaderPointerUp(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    releaseHeaderPointerCapture(e);
  }

  function onHeaderPointerCancel(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    releaseHeaderPointerCapture(e);
  }

  function onHeaderLostPointerCapture(): void {
    dragging = false;
  }

  function releaseHeaderPointerCapture(e: PointerEvent): void {
    const target = e.currentTarget as Element;
    if (target.hasPointerCapture(e.pointerId)) {
      target.releasePointerCapture(e.pointerId);
    }
  }

  $effect(() => {
    const open = $libraryVariablePrompt.open;
    if (open && !wasOpen) {
      position = defaultPosition();
      requestAnimationFrame(() => firstInputEl?.focus());
    } else if (!open) {
      position = null;
      dragging = false;
    }
    wasOpen = open;
  });
</script>

<svelte:window onkeydown={handleKeydown} onresize={onWindowResize} />

{#if $libraryVariablePrompt.open}
  {@const panelPosition = position ?? defaultPosition()}
  <div
    bind:this={panelEl}
    class="ui-dialog fixed z-[70] flex max-w-[calc(100vw-32px)] flex-col overflow-hidden rounded-2xl"
    style="top: {panelPosition.y}px; left: {panelPosition.x}px; width: {PANEL_WIDTH}px;"
    role="dialog"
    aria-modal="false"
    aria-labelledby="library-variable-title"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="flex select-none items-center gap-3 border-b border-border-subtle bg-bg-surface/55 px-4 py-3 {dragging ? 'cursor-grabbing' : 'cursor-grab'}"
      onpointerdown={onHeaderPointerDown}
      onpointermove={onHeaderPointerMove}
      onpointerup={onHeaderPointerUp}
      onpointercancel={onHeaderPointerCancel}
      onlostpointercapture={onHeaderLostPointerCapture}
    >
      <button
        type="button"
        class="cursor-pointer rounded border border-transparent bg-transparent p-1 text-text-muted transition-colors hover:border-border-subtle hover:bg-bg-hover hover:text-text-primary"
        aria-label="Cancel Library variable entry"
        title="Cancel"
        onclick={cancelLibraryVariablePrompt}
      >
        <X size={14} />
      </button>
      <div class="min-w-0 flex-1">
        <div id="library-variable-title" class="truncate text-sm font-semibold text-text-primary">
          {$libraryVariablePrompt.title}
        </div>
        <div class="mt-0.5 text-[11px] text-text-muted">
          {$libraryVariablePrompt.variables.length}
          {$libraryVariablePrompt.variables.length === 1 ? "variable" : "variables"}
        </div>
      </div>
    </div>

    <div class="p-4">
      <div class="space-y-3">
        {#each $libraryVariablePrompt.variables as variable, index (variable.name)}
          <label class="block">
            <span class="mb-1.5 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              <span>{variable.label ?? variable.name}</span>
              {#if variable.required}
                <span class="rounded bg-yellow/10 px-1.5 py-0.5 text-[9px] text-yellow">required</span>
              {/if}
            </span>
            {#if (variable.valueType ?? "string") === "select"}
              <select
                use:firstInput={index === 0}
                name={variable.name}
                value={$libraryVariablePrompt.values[variable.name] ?? ""}
                onchange={(e) => setLibraryVariableValue(variable.name, e.currentTarget.value)}
                class="w-full rounded border border-border bg-bg-deep px-3 py-2 font-mono text-sm text-text-primary outline-none focus:border-accent-dim {$libraryVariablePrompt.errors[variable.name] ? 'border-red/50' : ''}"
              >
                {#if !variable.required}
                  <option value="">None</option>
                {/if}
                {#each variable.options ?? [] as option}
                  <option value={option}>{option}</option>
                {/each}
              </select>
            {:else}
              <input
                use:firstInput={index === 0}
                name={variable.name}
                type={(variable.valueType ?? "string") === "int" || (variable.valueType ?? "string") === "float" ? "number" : "text"}
                step={(variable.valueType ?? "string") === "int" ? "1" : (variable.valueType ?? "string") === "float" ? "any" : undefined}
                value={$libraryVariablePrompt.values[variable.name] ?? ""}
                oninput={(e) => setLibraryVariableValue(variable.name, e.currentTarget.value)}
                class="w-full rounded border border-border bg-bg-deep px-3 py-2 font-mono text-sm text-text-primary outline-none placeholder:text-text-muted focus:border-accent-dim {$libraryVariablePrompt.errors[variable.name] ? 'border-red/50' : ''}"
                placeholder={variable.default ?? variable.name}
              />
            {/if}
            {#if $libraryVariablePrompt.errors[variable.name]}
              <div class="mt-1 text-[11px] text-red">{$libraryVariablePrompt.errors[variable.name]}</div>
            {/if}
          </label>
        {/each}
      </div>

      <div class="mt-4 flex justify-end gap-2">
        <button
          type="button"
          class="rounded border border-border-subtle bg-bg-elevated px-3 py-1.5 text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary"
          onclick={cancelLibraryVariablePrompt}
        >
          Cancel
        </button>
        <button
          type="button"
          class="rounded border border-accent-dim/40 bg-accent-dim/20 px-3 py-1.5 text-xs font-semibold text-accent hover:bg-accent-dim/40"
          onclick={submit}
        >
          Send
        </button>
      </div>
    </div>
  </div>
{/if}
