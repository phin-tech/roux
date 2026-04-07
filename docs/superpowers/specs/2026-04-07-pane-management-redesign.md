# Pane Management Redesign

## Problem

Pane focus and keyboard input break after split/restructure operations. The root cause is coupling between Svelte's component lifecycle and an imperative terminal registration side-channel (`paneTerminals` Map). When the tree restructures, Svelte destroys and recreates components, which unregisters terminals synchronously but re-registers them asynchronously and conditionally. The gap between unregister and re-register is a window where clicks can't route keyboard input.

The tree data model itself is sound. The problem is that terminal instances, focus state, and layout structure are entangled across three different systems (component lifecycle, registry Maps, store tree) with no clear owner.

## Design Principle

**Model owns the truth, view is disposable.**

Pane instances (terminals, PTY connections, metadata) live in a flat map with an explicit lifecycle: created on split/open, destroyed on explicit close. Svelte components are pure renderers that attach/detach DOM elements — destroying a component has zero effect on the terminal instance.

## Data Model

Three stores with distinct responsibilities:

### Pane Instances (the Model)

A reactive store wrapping `Map<string, PaneInstance>`. The store is a Svelte writable so that metadata changes (rename, cwd update, status) trigger re-renders in components that read it.

```typescript
interface PaneInstance {
  id: string
  type: "claude" | "shell" | "command" | "markdown"
  ptyId: string
  terminal: Terminal | null
  fitAddon: FitAddon | null
  outputChannel: Channel<PtyOutputPayload> | null
  unlisteners: UnlistenFn[]
  name?: string
  workingDir?: string
  command?: string
  docPath?: string
  // Per-type runtime state
  commandStatus?: "running" | "exited" | "error"
  commandExitCode?: number
  commandElapsed?: number
  rerunHandle?: { trigger: () => void; cleanup: () => void }
}

const paneInstances = writable<Map<string, PaneInstance>>(new Map())
```

A `PaneInstance` is created when a pane is born (split, open doc, run command) and destroyed only when explicitly closed. Tree restructuring never touches it.

Components read instance metadata reactively:
```typescript
// In PaneShell — reactive, triggers re-render on name/cwd/status changes
const instance = $derived($paneInstances.get(paneId))
```

### Session Layouts (the View)

A `Map<string, LayoutNode>` that describes how panes are arranged, referencing them by ID:

```typescript
type LayoutNode =
  | { kind: "leaf"; paneId: string }
  | { kind: "split"; direction: "h" | "v"; children: LayoutNode[];
      sizes?: number[]; stacked?: boolean; activeIndex?: number }
```

The layout tree has no instance data. To know what a leaf is, look up the pane instance by ID.

### Focus — Logical vs DOM

Two separate concerns to respect WebKit's native keyboard routing:

```typescript
const focusedPaneId = writable<string | null>(null)

/**
 * Set logical focus: which pane owns keyboard input.
 * Updates disableStdin on all terminals. Does NOT call terminal.focus()
 * — that must only happen from direct pointer event handlers to avoid
 * breaking WebKit/WKWebView native keyboard routing.
 */
function setLogicalFocus(paneId: string | null) {
  focusedPaneId.set(paneId)
  const instances = get(paneInstances)
  for (const [id, instance] of instances) {
    if (!instance.terminal) continue
    instance.terminal.options.disableStdin = (id !== paneId)
  }
}

/**
 * Request DOM focus on a pane's terminal. Only call from pointer event
 * handlers (mousedown, click). Never from keyboard navigation, split
 * operations, or restore flows.
 */
function requestDomFocus(paneId: string) {
  const instances = get(paneInstances)
  instances.get(paneId)?.terminal?.focus()
}
```

Click handlers call both: `setLogicalFocus(paneId)` then `requestDomFocus(paneId)`.
Keyboard navigation, split, close, restore only call `setLogicalFocus`.

## Pane Lifecycle

### Create

Called by commands (split, new shell, run command, open doc). Terminal is created eagerly, before any component mounts:

```typescript
function createPane(opts: CreatePaneOpts): string {
  const id = crypto.randomUUID()
  let terminal: Terminal | null = null
  let fitAddon: FitAddon | null = null

  if (opts.type !== "markdown") {
    terminal = new Terminal({ ...terminalSettings })
    fitAddon = new FitAddon()
    terminal.loadAddon(fitAddon)
  }

  paneInstances.update(map => {
    map.set(id, { id, terminal, fitAddon, outputChannel: null, unlisteners: [], ...opts })
    return new Map(map)
  })
  return id
}
```

### Dispose

Idempotent. Called only by explicit close (close pane, close session). Safe to call multiple times (e.g., PTY exit race with explicit close):

```typescript
function disposePane(id: string) {
  const instances = get(paneInstances)
  const instance = instances.get(id)
  if (!instance) return  // already disposed — idempotent
  for (const unlisten of instance.unlisteners) unlisten()
  instance.rerunHandle?.cleanup()
  instance.terminal?.dispose()
  paneInstances.update(map => {
    map.delete(id)
    return new Map(map)
  })
  // Kill PTY for shell/command types
}
```

### Replace PTY

For claude reconnect and command rerun — tears down old PTY listeners and attaches new ones without destroying the terminal:

```typescript
function replacePty(paneId: string, newPtyId: string) {
  paneInstances.update(map => {
    const instance = map.get(paneId)
    if (!instance) return map
    // Tear down old listeners
    for (const unlisten of instance.unlisteners.splice(0)) unlisten()
    instance.outputChannel = null
    // Update ptyId — new listeners will be attached by the caller
    instance.ptyId = newPtyId
    return new Map(map)
  })
}
```

### Attach / Detach DOM

Called by Svelte components based on visibility. Purely visual — no terminal creation or disposal:

```typescript
function attachToContainer(paneId: string, container: HTMLDivElement) {
  const instance = get(paneInstances).get(paneId)
  if (!instance?.terminal) return
  if (!instance.terminal.element) {
    // First open — suppress WebKit focus theft
    const origFocus = HTMLTextAreaElement.prototype.focus
    HTMLTextAreaElement.prototype.focus = function () {}
    try { instance.terminal.open(container) }
    finally { HTMLTextAreaElement.prototype.focus = origFocus }
  } else if (!container.contains(instance.terminal.element)) {
    container.appendChild(instance.terminal.element)
  }
  instance.fitAddon?.fit()
}

function detachFromContainer(paneId: string) {
  const instance = get(paneInstances).get(paneId)
  if (instance?.terminal?.element?.parentElement) {
    instance.terminal.element.remove()
  }
}
```

## Layout Operations

All operations are pure functions on `LayoutNode`. No side effects, no terminal touching.

### Split

Create pane (model), then insert into layout (view). Rollback on failure:

```typescript
function splitPane(sessionId: string, direction: "h" | "v", newPaneOpts) {
  const newPaneId = createPane(newPaneOpts)

  let inserted = false
  sessionLayouts.update(layouts => {
    const tree = layouts.get(sessionId)
    if (!tree) return layouts
    const focused = get(focusedPaneId)
    layouts.set(sessionId, insertLeaf(tree, focused, direction, newPaneId))
    inserted = true
    return new Map(layouts)
  })

  if (!inserted) {
    // Layout insert failed — clean up orphaned pane
    disposePane(newPaneId)
    return
  }

  setLogicalFocus(newPaneId)
}
```

### Close

Remove from layout, then dispose. Idempotent:

```typescript
function closePane(sessionId: string, paneId: string) {
  sessionLayouts.update(layouts => {
    const tree = layouts.get(sessionId)
    if (!tree) return layouts
    const result = removeLeaf(tree, paneId)
    if (result) layouts.set(sessionId, result)
    else layouts.delete(sessionId)
    return new Map(layouts)
  })

  disposePane(paneId)

  if (get(focusedPaneId) === paneId) {
    const tree = get(sessionLayouts).get(sessionId)
    setLogicalFocus(tree ? firstLeafId(tree) : null)
  }
}
```

### Close Session

Walks all pane IDs in the layout and disposes each before dropping the layout:

```typescript
function closeSessionPanes(sessionId: string) {
  const tree = get(sessionLayouts).get(sessionId)
  if (tree) {
    const paneIds = collectLeafIds(tree)
    for (const id of paneIds) disposePane(id)
  }
  sessionLayouts.update(layouts => {
    layouts.delete(sessionId)
    return new Map(layouts)
  })
}
```

### Navigate, Move, Resize, Stack, Fullscreen

All pure layout tree transforms. They never touch `paneInstances`:

- **Navigate**: read tree to find neighbor pane ID, call `setLogicalFocus`
- **Move**: remove leaf, re-insert at new position (swap, enter, extract)
- **Resize**: update `sizes` array on parent split
- **Stack**: toggle `stacked`/`activeIndex` on split node
- **Fullscreen**: separate `fullscreenPaneId` store, layout tree unchanged

Same algorithms as today, operating on a tree of IDs instead of a tree carrying instance data.

## Svelte Components

### SplitPane.svelte

Pure renderer, dramatically simpler:

```svelte
{#if node.kind === "leaf"}
  <PaneShell paneId={node.paneId} {sessionId} {visible} />

{:else if node.stacked}
  <!-- tab headers -->
  {#each node.children as child, i}
    <div class:hidden={i !== (node.activeIndex ?? 0)}>
      <svelte:self node={child} {sessionId}
        visible={visible && i === (node.activeIndex ?? 0)} />
    </div>
  {/each}

{:else}
  {#each node.children as child, i}
    <div style="flex: {node.sizes?.[i] ?? 1}">
      <svelte:self node={child} {sessionId} {visible} />
    </div>
  {/each}
{/if}
```

### PaneShell.svelte

Thin wrapper with visibility-aware attach/detach:

```svelte
<script>
  let { paneId, sessionId, visible = true } = $props()
  let container: HTMLDivElement
  const instance = $derived($paneInstances.get(paneId))

  onMount(() => {
    if (visible) attachToContainer(paneId, container)
    return () => detachFromContainer(paneId)
  })

  // Visibility-driven attach/detach — handles stacked tabs,
  // fullscreen, inactive sessions
  $effect(() => {
    if (visible) {
      attachToContainer(paneId, container)
    } else {
      detachFromContainer(paneId)
    }
  })

  function handleMouseDown() {
    setLogicalFocus(paneId)
    requestDomFocus(paneId)
  }
</script>

<!-- Title bar -->
<div class="title-bar">
  <span>{instance?.name ?? instance?.type}</span>
  <!-- close button, rename, etc — all read from reactive instance -->
</div>

<!-- Terminal container or type-specific UI -->
{#if instance?.type === "claude" && isDisconnected}
  <SessionPicker ... />
{:else if instance?.type === "markdown"}
  <LazyMarkdownPane docPath={instance?.docPath ?? ""} />
{:else if instance?.type === "command"}
  <div bind:this={container} onmousedown={handleMouseDown} />
  <!-- rerun button, status — read from instance.commandStatus etc -->
{:else}
  <div bind:this={container} onmousedown={handleMouseDown} />
{/if}
```

### Resize Handling

`PaneShell` owns a `ResizeObserver` on its container. When visible and the container resizes, it calls `instance.fitAddon.fit()` via the existing `resizeScheduler`. This replaces the per-component resize logic scattered across `Terminal.svelte` and `ShellTerminal.svelte`.

## PTY Output Attachment

PTY output listeners are owned by the `PaneInstance`, not by components. They're set up at `createPane` time (for shells/commands) or at session init time (for claude terminals). Since the instance outlives component mount/unmount cycles, there's no listener re-attachment needed on tree restructure.

For reconnect/rerun flows, `replacePty` tears down old listeners before the caller attaches new ones.

## Layout Persistence

Two things are persisted to localStorage:

1. **Layout tree** (`sessionLayouts`) — the structural arrangement of pane IDs
2. **Pane descriptors** — serializable subset of `PaneInstance` needed to recreate panes on restore:

```typescript
interface PaneDescriptor {
  id: string
  type: "claude" | "shell" | "command" | "markdown"
  ptyId: string
  name?: string
  workingDir?: string
  command?: string
  docPath?: string
}
```

On save: extract descriptors from `paneInstances` for all panes in the layout. On restore:

1. Load layout tree and pane descriptors from localStorage
2. Strip command panes (their processes are gone)
3. For each remaining descriptor, call `createPane` with fresh PTY IDs for shells
4. Claude panes reuse their session ID as PTY ID (same as today)
5. Insert the restored layout tree into `sessionLayouts`

The layout tree itself remains pure IDs — the descriptors are a separate persisted store that provides the metadata needed to reconstruct `PaneInstance`s.

## What Gets Deleted

- `terminalRegistry.ts` — the entire `paneTerminals` focus map, `claudeTerminals` map, `shellTerminals` map, `ensureClaudeTerminal`, `ensureShellTerminal`, register/unregister/dispose functions. All replaced by `paneInstances`.
- `Terminal.svelte` lifecycle machinery — `attach/detach`, `destroyed` flag, `capturedSessionId` pattern, `getOrCreateTerminal`, WebKit focus suppression per-component. Replaced by `PaneShell` + `attachToContainer`.
- `ShellTerminal.svelte` lifecycle machinery — same cleanup.
- `CommandPane.svelte` lifecycle machinery — same cleanup.
- Focus logic scattered across components — replaced by `setLogicalFocus` + `requestDomFocus`.
- `{#key node.pane.id}` block in `SplitPane.svelte` — no longer needed since components don't own terminal state.

## What Stays

- The tree transform algorithms (split flattening, remove with collapse, navigate, move, resize) — these are correct, just need to operate on `LayoutNode` with pane ID references instead of embedded `Pane` objects.
- Stacking/fullscreen model.
- Drag-and-drop pane movement.
- Command registry and keybindings.
- `resizeScheduler.ts` (unchanged).

## File Structure

```
src/lib/panes/
  instances.ts       — PaneInstance type, paneInstances store, create/dispose/replacePty/attach/detach
  focus.ts           — focusedPaneId store, setLogicalFocus, requestDomFocus, fullscreenPaneId
  layout.ts          — LayoutNode type, sessionLayouts store, all tree transforms
  persistence.ts     — PaneDescriptor type, localStorage save/restore for layouts + descriptors
  resizeScheduler.ts — (unchanged)

src/lib/components/
  SplitPane.svelte   — recursive layout renderer (simplified)
  PaneShell.svelte   — thin terminal container (new, replaces Terminal/ShellTerminal wrappers)
  SessionPicker.svelte — (unchanged, shown when claude session disconnected)
```

`Terminal.svelte` and `ShellTerminal.svelte` are deleted as separate components. Their terminal-specific logic (PTY output attachment, onData handlers) moves into `instances.ts` (setup) and `PaneShell.svelte` (display).

## Type-Specific Rendering in PaneShell

`PaneShell` handles type-specific UI via `{#if}` on the pane type, reading from the reactive instance store:

- **claude**: show `SessionPicker` overlay when session status is disconnected, terminal container otherwise
- **shell**: terminal container + OSC 7 handler (registered once at `createPane` time on the terminal's parser)
- **command**: terminal container + status/rerun controls (read from `instance.commandStatus`, `instance.rerunHandle`)
- **markdown**: `LazyMarkdownPane` (no terminal at all)

The key point: this is rendering logic only. No lifecycle management, no registration. The `PaneInstance` already exists and owns all state.
