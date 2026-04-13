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
 * A small per-pty ring buffer replays recent bytes to new subscribers so
 * that a subscriber attaching shortly after the shell boots doesn't miss
 * the initial flush (which in practice arrives in one big chunk the
 * moment the backend's pending-output channel is attached).
 */

type Listener = (bytes: Uint8Array) => void;

const listeners = new Map<string, Set<Listener>>();
const buffers = new Map<string, Uint8Array>();

const BUFFER_CAP = 16 * 1024;

function appendBytes(ptyId: string, bytes: Uint8Array): void {
  const prev = buffers.get(ptyId);
  if (!prev) {
    buffers.set(
      ptyId,
      bytes.length <= BUFFER_CAP
        ? new Uint8Array(bytes)
        : new Uint8Array(bytes.subarray(bytes.length - BUFFER_CAP)),
    );
    return;
  }
  const total = prev.length + bytes.length;
  if (total <= BUFFER_CAP) {
    const merged = new Uint8Array(total);
    merged.set(prev, 0);
    merged.set(bytes, prev.length);
    buffers.set(ptyId, merged);
    return;
  }
  const merged = new Uint8Array(BUFFER_CAP);
  const incomingKept = Math.min(bytes.length, BUFFER_CAP);
  const prevKept = BUFFER_CAP - incomingKept;
  merged.set(prev.subarray(prev.length - prevKept), 0);
  merged.set(bytes.subarray(bytes.length - incomingKept), prevKept);
  buffers.set(ptyId, merged);
}

export function emitPtyOutput(ptyId: string, bytes: Uint8Array): void {
  appendBytes(ptyId, bytes);
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

  // Replay recent history so a late subscriber still sees bytes that
  // arrived before it attached. Wrapped in try/catch for the same reason
  // as emit.
  const recent = buffers.get(ptyId);
  if (recent && recent.length > 0) {
    try {
      cb(recent);
    } catch {
      // ignore
    }
  }

  return () => {
    const s = listeners.get(ptyId);
    if (!s) return;
    s.delete(cb);
    if (s.size === 0) listeners.delete(ptyId);
  };
}

/**
 * Drop the replay buffer for a ptyId. Call when a PTY is being respawned
 * under the same id (reconnect, rerun) so a subsequent `onPtyOutput`
 * subscriber doesn't see stale bytes from the prior process.
 */
export function clearPtyOutputBuffer(ptyId: string): void {
  buffers.delete(ptyId);
}

/** Test-only: wipe all state. */
export function resetPtyOutputBus(): void {
  listeners.clear();
  buffers.clear();
}
