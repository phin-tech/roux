use std::collections::VecDeque;

pub const PTY_BACKLOG_LIMIT_BYTES: usize = 256 * 1024;

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
}
