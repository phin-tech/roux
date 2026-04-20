/**
 * Fan-out bus for PTY output bytes, keyed by ptyId.
 *
 * The xterm writer in `terminals.ts` is the primary consumer of each PTY's
 * output channel, but other code (readiness detection, sniffers, tests)
 * needs to observe the same stream without taking over the channel. This
 * module sits next to the channel callback: `emitPtyOutput` is called once
 * per chunk alongside the xterm write, and any number of subscribers can
 * tap in via `onPtyOutput`.
 *
 * Replay is now handled by the backend PtyLogger ring buffer - this module
 * is purely fan-out, no local buffering.
 */

type Listener = (bytes: Uint8Array) => void;

const listeners = new Map<string, Set<Listener>>();

export function emitPtyOutput(ptyId: string, bytes: Uint8Array): void {
  const set = listeners.get(ptyId);
  if (!set) return;
  for (const cb of set) {
    try {
      cb(bytes);
    } catch {
      // Subscribers must not break the fan-out; swallow and continue.
    }
  }
}

export function onPtyOutput(ptyId: string, cb: Listener): () => void {
  let set = listeners.get(ptyId);
  if (!set) {
    set = new Set();
    listeners.set(ptyId, set);
  }
  set.add(cb);

  return () => {
    const s = listeners.get(ptyId);
    if (!s) return;
    s.delete(cb);
    if (s.size === 0) listeners.delete(ptyId);
  };
}

/**
 * No-op for backward compatibility. Replay is now handled by backend.
 * @deprecated Backend PtyLogger handles replay now
 */
export function clearPtyOutputBuffer(_ptyId: string): void {
  // No-op: backend handles replay buffer
}

/** Test-only: wipe all state. */
export function resetPtyOutputBus(): void {
  listeners.clear();
}
