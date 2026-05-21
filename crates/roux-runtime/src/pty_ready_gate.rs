//! Write-side gate that holds bytes destined for a freshly-spawned shell
//! PTY until the shell's prompt is ready to accept keystrokes.
//!
//! See `src/lib/panes/shellReady.ts` for the historical frontend version;
//! this module exists so the invariant ("don't write into a shell that
//! hasn't drawn its prompt yet") is enforced by the PTY layer regardless
//! of which frontend path issued the write.
//!
//! The gate is a pure state machine parameterised on `Instant`, so it can
//! be unit-tested without spawning a real shell.

use std::time::{Duration, Instant};

/// ESC ] 1 3 3 ; A — the standard OSC 133 "prompt start" marker.
const OSC_133_A: &[u8] = b"\x1b]133;A";

/// How many trailing bytes of observed output to keep so we can find an
/// OSC 133;A that straddles a chunk boundary. Must be at least
/// `OSC_133_A.len() - 1`.
const SCAN_TAIL: usize = OSC_133_A.len() - 1;

/// Upper bound on how many bytes the gate will buffer while gating. In
/// practice the buffer holds a profile's `cd`/`export`/setup/startup
/// lines — a few hundred bytes. This cap exists to defend against a
/// large paste landing in a new pane before the shell is ready; we'd
/// rather drop the oldest bytes than grow unbounded until the 5s
/// timeout expires.
pub(crate) const BUFFER_CAP_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Gating,
    Open,
}

pub struct ShellReadyGate {
    state: State,
    quiet: Duration,
    timeout: Duration,
    created_at: Instant,
    last_output_at: Option<Instant>,
    buffer: Vec<u8>,
    scan_tail: Vec<u8>,
}

impl ShellReadyGate {
    pub fn new(now: Instant, quiet: Duration, timeout: Duration) -> Self {
        Self {
            state: State::Gating,
            quiet,
            timeout,
            created_at: now,
            last_output_at: None,
            buffer: Vec::new(),
            scan_tail: Vec::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state, State::Open)
    }

    /// Feed shell stdout bytes to the gate. Returns bytes that should now
    /// be flushed to the PTY writer (only non-empty when this call opened
    /// the gate and a write buffer had accumulated).
    pub fn on_output(&mut self, bytes: &[u8], now: Instant) -> Vec<u8> {
        if matches!(self.state, State::Open) {
            return Vec::new();
        }
        if bytes.is_empty() {
            return Vec::new();
        }
        self.last_output_at = Some(now);

        if self.scan_has_osc133(bytes) {
            return self.open();
        }
        self.update_scan_tail(bytes);
        Vec::new()
    }

    /// A write arrived. Returns the bytes that should actually be sent to
    /// the PTY: empty if still gating, or buffered+data if this call
    /// opened the gate via a time-based predicate.
    pub fn on_write(&mut self, data: &[u8], now: Instant) -> Vec<u8> {
        if matches!(self.state, State::Open) {
            return data.to_vec();
        }
        if self.should_open_by_time(now) {
            self.append_bounded(data);
            return self.open();
        }
        self.append_bounded(data);
        Vec::new()
    }

    /// Append to the gate's write buffer, dropping oldest bytes if we
    /// would exceed [`BUFFER_CAP_BYTES`]. If the incoming chunk is
    /// itself larger than the cap, only the last `BUFFER_CAP_BYTES` of
    /// it are retained.
    fn append_bounded(&mut self, data: &[u8]) {
        if data.len() >= BUFFER_CAP_BYTES {
            self.buffer.clear();
            self.buffer.extend_from_slice(&data[data.len() - BUFFER_CAP_BYTES..]);
            return;
        }
        let combined = self.buffer.len() + data.len();
        if combined > BUFFER_CAP_BYTES {
            let drop = combined - BUFFER_CAP_BYTES;
            self.buffer.drain(..drop);
        }
        self.buffer.extend_from_slice(data);
    }

    /// Called periodically by the owner. If a time predicate now says the
    /// shell is ready, returns the buffered bytes so the owner can flush
    /// them.
    pub fn poll(&mut self, now: Instant) -> Vec<u8> {
        if matches!(self.state, State::Open) {
            return Vec::new();
        }
        if self.should_open_by_time(now) {
            return self.open();
        }
        Vec::new()
    }

    fn should_open_by_time(&self, now: Instant) -> bool {
        if now.saturating_duration_since(self.created_at) >= self.timeout {
            return true;
        }
        if let Some(last) = self.last_output_at {
            if now.saturating_duration_since(last) >= self.quiet {
                return true;
            }
        }
        false
    }

    fn open(&mut self) -> Vec<u8> {
        self.state = State::Open;
        std::mem::take(&mut self.buffer)
    }

    fn scan_has_osc133(&self, bytes: &[u8]) -> bool {
        // Concatenate prior tail + new bytes for straddle-aware scanning.
        // The scan window is small (at most SCAN_TAIL + bytes.len()), so we
        // avoid allocating when the new chunk alone is clearly enough.
        if self.scan_tail.is_empty() {
            return find_subsequence(bytes, OSC_133_A).is_some();
        }
        let mut joined = Vec::with_capacity(self.scan_tail.len() + bytes.len());
        joined.extend_from_slice(&self.scan_tail);
        joined.extend_from_slice(bytes);
        find_subsequence(&joined, OSC_133_A).is_some()
    }

    fn update_scan_tail(&mut self, bytes: &[u8]) {
        if bytes.len() >= SCAN_TAIL {
            self.scan_tail.clear();
            self.scan_tail.extend_from_slice(&bytes[bytes.len() - SCAN_TAIL..]);
        } else {
            // Append and truncate from the front.
            self.scan_tail.extend_from_slice(bytes);
            if self.scan_tail.len() > SCAN_TAIL {
                let drop = self.scan_tail.len() - SCAN_TAIL;
                self.scan_tail.drain(..drop);
            }
        }
    }
}

fn find_subsequence(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(now: Instant) -> ShellReadyGate {
        ShellReadyGate::new(now, Duration::from_millis(200), Duration::from_secs(5))
    }

    #[test]
    fn new_gate_is_gating() {
        let g = gate(Instant::now());
        assert!(!g.is_open());
    }

    #[test]
    fn write_while_gating_buffers_and_returns_empty() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let out = g.on_write(b"claude\n", t0);
        assert_eq!(out, Vec::<u8>::new());
        assert!(!g.is_open());
    }

    #[test]
    fn osc133_in_output_opens_gate_and_flushes_buffer() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_write(b"claude\n", t0);
        let flush = g.on_output(b"welcome\x1b]133;A\x07$ ", t0);
        assert!(g.is_open());
        assert_eq!(flush, b"claude\n");
    }

    #[test]
    fn osc133_with_no_buffered_writes_opens_gate_with_empty_flush() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let flush = g.on_output(b"\x1b]133;A\x07", t0);
        assert!(g.is_open());
        assert_eq!(flush, Vec::<u8>::new());
    }

    #[test]
    fn osc133_straddling_chunks_is_detected() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        // Split OSC marker across two chunks.
        let _ = g.on_output(b"prefix \x1b]13", t0);
        assert!(!g.is_open());
        let _ = g.on_output(b"3;A\x07 rest", t0);
        assert!(g.is_open());
    }

    #[test]
    fn once_open_writes_pass_through() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_output(b"\x1b]133;A", t0);
        let out = g.on_write(b"claude\n", t0);
        assert_eq!(out, b"claude\n");
    }

    #[test]
    fn quiescence_opens_gate_after_quiet_window() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_write(b"claude\n", t0);
        // Shell emits its prompt (no OSC 133) at t+10ms.
        let _ = g.on_output(b"$ ", t0 + Duration::from_millis(10));
        assert!(!g.is_open());
        // Still within quiet window.
        let flush = g.poll(t0 + Duration::from_millis(100));
        assert_eq!(flush, Vec::<u8>::new());
        assert!(!g.is_open());
        // Past quiet window after last output.
        let flush = g.poll(t0 + Duration::from_millis(215));
        assert!(g.is_open());
        assert_eq!(flush, b"claude\n");
    }

    #[test]
    fn quiescence_resets_on_new_output() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_output(b"rc line 1\n", t0 + Duration::from_millis(10));
        // Almost past quiet — but another chunk arrives.
        let _ = g.on_output(b"rc line 2\n", t0 + Duration::from_millis(190));
        // Would have been past quiet from the first chunk but not the second.
        let flush = g.poll(t0 + Duration::from_millis(250));
        assert!(!g.is_open());
        assert_eq!(flush, Vec::<u8>::new());
        // Now past quiet from the second chunk.
        let flush = g.poll(t0 + Duration::from_millis(400));
        assert!(g.is_open());
        assert_eq!(flush, Vec::<u8>::new());
    }

    #[test]
    fn hard_timeout_opens_gate_even_with_no_output() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_write(b"claude\n", t0);
        let flush = g.poll(t0 + Duration::from_secs(4));
        assert!(!g.is_open());
        assert_eq!(flush, Vec::<u8>::new());
        let flush = g.poll(t0 + Duration::from_secs(6));
        assert!(g.is_open());
        assert_eq!(flush, b"claude\n");
    }

    #[test]
    fn write_itself_opens_gate_if_time_predicate_fires() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_output(b"$ ", t0 + Duration::from_millis(10));
        // A write arriving past the quiet window opens the gate on the
        // write itself — the owner doesn't need a separate poll.
        let out = g.on_write(b"claude\n", t0 + Duration::from_millis(300));
        assert!(g.is_open());
        assert_eq!(out, b"claude\n");
    }

    #[test]
    fn buffered_writes_are_capped_oldest_bytes_drop_first() {
        let t0 = Instant::now();
        let mut g = ShellReadyGate::new(t0, Duration::from_millis(200), Duration::from_secs(5));
        // Fill well past the cap. Each chunk is distinguishable so we can
        // verify which bytes survived.
        let big = vec![b'A'; BUFFER_CAP_BYTES];
        let _ = g.on_write(&big, t0);
        let tail = b"LATEST\n";
        let _ = g.on_write(tail, t0);
        // Gate opens; flushed bytes are at most cap, and include the most
        // recent write (oldest bytes dropped).
        let flushed = g.on_output(b"\x1b]133;A", t0);
        assert!(flushed.len() <= BUFFER_CAP_BYTES, "buffer exceeded cap: {}", flushed.len(),);
        assert!(flushed.ends_with(tail), "most recent write should survive the cap",);
    }

    #[test]
    fn multiple_buffered_writes_flush_in_order() {
        let t0 = Instant::now();
        let mut g = gate(t0);
        let _ = g.on_write(b"cd /tmp\n", t0);
        let _ = g.on_write(b"claude\n", t0);
        let flush = g.on_output(b"\x1b]133;A", t0);
        assert_eq!(flush, b"cd /tmp\nclaude\n");
    }
}
