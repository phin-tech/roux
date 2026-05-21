use std::collections::VecDeque;

use crate::pty_lifecycle::ExitReason;

pub const PTY_BACKLOG_LIMIT_BYTES: usize = 256 * 1024;
pub const PTY_FLUSH_INTERVAL_MS: u64 = 16;
pub const PTY_FLUSH_BATCH_LIMIT_BYTES: usize = 32 * 1024;

#[derive(Debug)]
pub struct PtyOutputBacklog {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    limit_bytes: usize,
}

impl PtyOutputBacklog {
    pub fn new(limit_bytes: usize) -> Self {
        Self { chunks: VecDeque::new(), bytes: 0, limit_bytes }
    }

    pub fn buffer(&mut self, bytes: Vec<u8>) {
        self.bytes += bytes.len();
        self.chunks.push_back(bytes);
        while self.bytes > self.limit_bytes {
            let Some(removed) = self.chunks.pop_front() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.len());
        }
    }

    pub fn pop_front(&mut self) -> Option<Vec<u8>> {
        let bytes = self.chunks.pop_front()?;
        self.bytes = self.bytes.saturating_sub(bytes.len());
        Some(bytes)
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn len_bytes(&self) -> usize {
        self.bytes
    }
}

impl Default for PtyOutputBacklog {
    fn default() -> Self {
        Self::new(PTY_BACKLOG_LIMIT_BYTES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyOutputDeliveryKind {
    Live,
    Backlog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputDelivery {
    pub kind: PtyOutputDeliveryKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct PtyOutputDeliveryState {
    attached: bool,
    backlog: PtyOutputBacklog,
}

impl PtyOutputDeliveryState {
    pub fn new() -> Self {
        Self { attached: false, backlog: PtyOutputBacklog::default() }
    }

    pub fn send(&mut self, bytes: Vec<u8>) -> Option<PtyOutputDelivery> {
        if self.attached {
            Some(PtyOutputDelivery { kind: PtyOutputDeliveryKind::Live, bytes })
        } else {
            self.backlog.buffer(bytes);
            None
        }
    }

    pub fn attach(&mut self) -> Option<PtyOutputDelivery> {
        self.attached = true;
        self.next_backlog_delivery()
    }

    pub fn delivery_succeeded(
        &mut self,
        kind: PtyOutputDeliveryKind,
    ) -> Option<PtyOutputDelivery> {
        if matches!(kind, PtyOutputDeliveryKind::Backlog) {
            self.next_backlog_delivery()
        } else {
            None
        }
    }

    pub fn delivery_failed(&mut self, delivery: PtyOutputDelivery) {
        self.attached = false;
        self.backlog.buffer(delivery.bytes);
    }

    fn next_backlog_delivery(&mut self) -> Option<PtyOutputDelivery> {
        self.backlog.pop_front().map(|bytes| PtyOutputDelivery {
            kind: PtyOutputDeliveryKind::Backlog,
            bytes,
        })
    }
}

impl Default for PtyOutputDeliveryState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PtyOutputChunk {
    Data(Vec<u8>),
    Eof,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PtyOutputFlushAction {
    Output(Vec<u8>),
    Exit(ExitReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyReaderStep<'a> {
    Data(&'a [u8]),
    Eof,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtyReaderPlan<'a> {
    pub observer_bytes: Option<&'a [u8]>,
    pub output_chunk: PtyOutputChunk,
    pub stop: bool,
}

pub fn plan_reader_step<'a>(step: PtyReaderStep<'a>) -> PtyReaderPlan<'a> {
    match step {
        PtyReaderStep::Data(bytes) => PtyReaderPlan {
            observer_bytes: Some(bytes),
            output_chunk: PtyOutputChunk::Data(bytes.to_vec()),
            stop: false,
        },
        PtyReaderStep::Eof => PtyReaderPlan {
            observer_bytes: None,
            output_chunk: PtyOutputChunk::Eof,
            stop: true,
        },
        PtyReaderStep::Error => PtyReaderPlan {
            observer_bytes: None,
            output_chunk: PtyOutputChunk::Error,
            stop: true,
        },
    }
}

#[derive(Debug, Clone)]
pub struct PtyOutputFlusher {
    batch: Vec<u8>,
    last_flush_ms: u64,
    flush_interval_ms: u64,
    batch_limit_bytes: usize,
    finished: bool,
}

impl PtyOutputFlusher {
    pub fn new(start_ms: u64) -> Self {
        Self::with_limits(start_ms, PTY_FLUSH_INTERVAL_MS, PTY_FLUSH_BATCH_LIMIT_BYTES)
    }

    pub fn with_limits(
        start_ms: u64,
        flush_interval_ms: u64,
        batch_limit_bytes: usize,
    ) -> Self {
        Self {
            batch: Vec::with_capacity(8192),
            last_flush_ms: start_ms,
            flush_interval_ms,
            batch_limit_bytes,
            finished: false,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn recv_timeout_ms(&self, now_ms: u64) -> Option<u64> {
        if self.batch.is_empty() || self.finished {
            return None;
        }
        let elapsed_ms = now_ms.saturating_sub(self.last_flush_ms);
        Some(self.flush_interval_ms.saturating_sub(elapsed_ms))
    }

    pub fn on_timeout(&mut self, now_ms: u64) -> Vec<PtyOutputFlushAction> {
        self.flush_batch(now_ms)
            .map(PtyOutputFlushAction::Output)
            .into_iter()
            .collect()
    }

    pub fn on_chunk(
        &mut self,
        chunk: PtyOutputChunk,
        now_ms: u64,
    ) -> Vec<PtyOutputFlushAction> {
        if self.finished {
            return Vec::new();
        }

        match chunk {
            PtyOutputChunk::Data(data) => {
                self.batch.extend_from_slice(&data);
                let elapsed_ms = now_ms.saturating_sub(self.last_flush_ms);
                if elapsed_ms >= self.flush_interval_ms
                    || self.batch.len() >= self.batch_limit_bytes
                {
                    self.on_timeout(now_ms)
                } else {
                    Vec::new()
                }
            }
            PtyOutputChunk::Eof => self.finish(ExitReason::Exit, now_ms),
            PtyOutputChunk::Error => self.finish(ExitReason::IoError, now_ms),
        }
    }

    fn finish(&mut self, reason: ExitReason, now_ms: u64) -> Vec<PtyOutputFlushAction> {
        self.finished = true;
        let mut actions = self.on_timeout(now_ms);
        actions.push(PtyOutputFlushAction::Exit(reason));
        actions
    }

    fn flush_batch(&mut self, now_ms: u64) -> Option<Vec<u8>> {
        if self.batch.is_empty() {
            return None;
        }
        self.last_flush_ms = now_ms;
        Some(std::mem::take(&mut self.batch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_and_drains_in_order() {
        let mut backlog = PtyOutputBacklog::new(1024);
        backlog.buffer(vec![1, 2, 3]);
        backlog.buffer(vec![4, 5, 6]);

        assert_eq!(backlog.len_bytes(), 6);
        assert_eq!(backlog.pop_front(), Some(vec![1, 2, 3]));
        assert_eq!(backlog.pop_front(), Some(vec![4, 5, 6]));
        assert_eq!(backlog.pop_front(), None);
        assert!(backlog.is_empty());
    }

    #[test]
    fn trims_oldest_chunks_when_limit_is_exceeded() {
        let mut backlog = PtyOutputBacklog::new(10);
        backlog.buffer(vec![1; 5]);
        backlog.buffer(vec![2; 5]);
        backlog.buffer(vec![3; 5]);

        assert_eq!(backlog.len_bytes(), 10);
        assert_eq!(backlog.pop_front(), Some(vec![2; 5]));
        assert_eq!(backlog.pop_front(), Some(vec![3; 5]));
        assert_eq!(backlog.pop_front(), None);
    }

    #[test]
    fn oversized_single_chunk_drops_backlog_to_preserve_limit() {
        let mut backlog = PtyOutputBacklog::new(4);
        backlog.buffer(vec![1, 2]);
        backlog.buffer(vec![3, 4, 5, 6, 7]);

        assert_eq!(backlog.len_bytes(), 0);
        assert_eq!(backlog.pop_front(), None);
    }

    #[test]
    fn delivery_state_buffers_until_attach_then_drains_backlog() {
        let mut state = PtyOutputDeliveryState::default();

        assert_eq!(state.send(vec![1, 2, 3]), None);
        assert_eq!(state.send(vec![4, 5, 6]), None);

        let first = state.attach().expect("first backlog delivery");
        assert_eq!(
            first,
            PtyOutputDelivery {
                kind: PtyOutputDeliveryKind::Backlog,
                bytes: vec![1, 2, 3],
            }
        );

        let second = state.delivery_succeeded(first.kind).expect("second backlog delivery");
        assert_eq!(
            second,
            PtyOutputDelivery {
                kind: PtyOutputDeliveryKind::Backlog,
                bytes: vec![4, 5, 6],
            }
        );
        assert_eq!(state.delivery_succeeded(second.kind), None);
    }

    #[test]
    fn delivery_state_sends_live_output_when_attached() {
        let mut state = PtyOutputDeliveryState::default();
        assert_eq!(state.attach(), None);

        let delivery = state.send(vec![9, 8, 7]).expect("live delivery");
        assert_eq!(
            delivery,
            PtyOutputDelivery {
                kind: PtyOutputDeliveryKind::Live,
                bytes: vec![9, 8, 7],
            }
        );
        assert_eq!(state.delivery_succeeded(delivery.kind), None);
    }

    #[test]
    fn delivery_state_rebuffers_failed_live_delivery() {
        let mut state = PtyOutputDeliveryState::default();
        assert_eq!(state.attach(), None);

        let delivery = state.send(vec![1, 2, 3]).expect("live delivery");
        state.delivery_failed(delivery);

        let retry = state.attach().expect("retry delivery");
        assert_eq!(
            retry,
            PtyOutputDelivery {
                kind: PtyOutputDeliveryKind::Backlog,
                bytes: vec![1, 2, 3],
            }
        );
    }

    #[test]
    fn delivery_state_rebuffers_failed_backlog_delivery_after_remaining_backlog() {
        let mut state = PtyOutputDeliveryState::default();
        assert_eq!(state.send(vec![1]), None);
        assert_eq!(state.send(vec![2]), None);
        assert_eq!(state.send(vec![3]), None);

        let first = state.attach().expect("first backlog delivery");
        assert_eq!(first.bytes, vec![1]);
        assert_eq!(first.kind, PtyOutputDeliveryKind::Backlog);
        assert_eq!(state.delivery_succeeded(first.kind).map(|d| d.bytes), Some(vec![2]));

        state.delivery_failed(PtyOutputDelivery {
            kind: PtyOutputDeliveryKind::Backlog,
            bytes: vec![2],
        });

        let next = state.attach().expect("remaining backlog delivery");
        assert_eq!(next.bytes, vec![3]);
        let retry = state.delivery_succeeded(next.kind).expect("failed chunk retry");
        assert_eq!(retry.bytes, vec![2]);
    }

    #[test]
    fn flusher_buffers_data_until_interval_or_size_threshold() {
        let mut flusher = PtyOutputFlusher::with_limits(0, 16, 8);

        assert_eq!(
            flusher.on_chunk(PtyOutputChunk::Data(vec![1, 2, 3]), 1),
            Vec::<PtyOutputFlushAction>::new()
        );
        assert_eq!(flusher.recv_timeout_ms(1), Some(15));

        assert_eq!(
            flusher.on_chunk(PtyOutputChunk::Data(vec![4, 5, 6]), 10),
            Vec::<PtyOutputFlushAction>::new()
        );
        assert_eq!(flusher.recv_timeout_ms(10), Some(6));

        assert_eq!(
            flusher.on_chunk(PtyOutputChunk::Data(vec![7, 8]), 11),
            vec![PtyOutputFlushAction::Output(vec![1, 2, 3, 4, 5, 6, 7, 8])]
        );
        assert_eq!(flusher.recv_timeout_ms(11), None);
    }

    #[test]
    fn flusher_flushes_batch_on_timeout() {
        let mut flusher = PtyOutputFlusher::with_limits(0, 16, 32);
        assert!(flusher
            .on_chunk(PtyOutputChunk::Data(vec![1, 2, 3]), 1)
            .is_empty());

        assert_eq!(
            flusher.on_timeout(17),
            vec![PtyOutputFlushAction::Output(vec![1, 2, 3])]
        );
        assert_eq!(flusher.recv_timeout_ms(17), None);
        assert_eq!(flusher.on_timeout(18), Vec::<PtyOutputFlushAction>::new());
    }

    #[test]
    fn flusher_finishes_after_flushing_pending_output() {
        let mut flusher = PtyOutputFlusher::with_limits(0, 16, 32);
        assert!(flusher
            .on_chunk(PtyOutputChunk::Data(vec![1, 2, 3]), 1)
            .is_empty());

        assert_eq!(
            flusher.on_chunk(PtyOutputChunk::Eof, 2),
            vec![
                PtyOutputFlushAction::Output(vec![1, 2, 3]),
                PtyOutputFlushAction::Exit(ExitReason::Exit),
            ]
        );
        assert!(flusher.is_finished());
    }

    #[test]
    fn flusher_maps_error_to_io_error_exit() {
        let mut flusher = PtyOutputFlusher::with_limits(0, 16, 32);

        assert_eq!(
            flusher.on_chunk(PtyOutputChunk::Error, 1),
            vec![PtyOutputFlushAction::Exit(ExitReason::IoError)]
        );
        assert!(flusher.is_finished());
    }

    #[test]
    fn reader_plan_for_data_feeds_observers_and_output() {
        let plan = plan_reader_step(PtyReaderStep::Data(b"abc"));

        assert_eq!(
            plan,
            PtyReaderPlan {
                observer_bytes: Some(&b"abc"[..]),
                output_chunk: PtyOutputChunk::Data(b"abc".to_vec()),
                stop: false,
            }
        );
    }

    #[test]
    fn reader_plan_for_eof_stops_after_eof_chunk() {
        let plan = plan_reader_step(PtyReaderStep::Eof);

        assert_eq!(
            plan,
            PtyReaderPlan {
                observer_bytes: None,
                output_chunk: PtyOutputChunk::Eof,
                stop: true,
            }
        );
    }

    #[test]
    fn reader_plan_for_error_stops_after_error_chunk() {
        let plan = plan_reader_step(PtyReaderStep::Error);

        assert_eq!(
            plan,
            PtyReaderPlan {
                observer_bytes: None,
                output_chunk: PtyOutputChunk::Error,
                stop: true,
            }
        );
    }
}
