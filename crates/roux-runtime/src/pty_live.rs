use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use portable_pty::MasterPty;
use thiserror::Error;

use crate::pty_output::{
    PtyOutputDelivery, PtyOutputDeliveryState,
};
use crate::pty_ready_gate::ShellReadyGate;
use crate::pty_registry::PtySessionRegistryEntry;
use crate::pty_session::PtySessionMetadata;

pub type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
pub type ReadyGate = Arc<Mutex<ShellReadyGate>>;

pub trait PtyOutputSink: Send + 'static {
    fn send_output(&self, bytes: Vec<u8>) -> bool;
}

pub trait PtyOutputLogger: Send + 'static {
    fn write_output(&mut self, bytes: &[u8]);
    fn recent_output(&self, max_bytes: usize) -> Vec<u8>;
}

struct PtyOutputState<Sink, Logger> {
    channel: Option<Sink>,
    delivery: PtyOutputDeliveryState,
    logger: Option<Arc<Mutex<Logger>>>,
}

impl<Sink, Logger> PtyOutputState<Sink, Logger> {
    fn new() -> Self {
        Self { channel: None, delivery: PtyOutputDeliveryState::default(), logger: None }
    }

    fn new_with_logger(logger: Arc<Mutex<Logger>>) -> Self {
        Self {
            channel: None,
            delivery: PtyOutputDeliveryState::default(),
            logger: Some(logger),
        }
    }
}

impl<Sink, Logger> PtyOutputState<Sink, Logger>
where
    Sink: PtyOutputSink,
    Logger: PtyOutputLogger,
{
    fn send_or_buffer(&mut self, bytes: Vec<u8>) {
        if let Some(ref logger) = self.logger {
            if let Ok(mut logger) = logger.lock() {
                logger.write_output(&bytes);
            }
        }
        if let Some(delivery) = self.delivery.send(bytes) {
            self.deliver(delivery);
        }
    }

    fn attach(&mut self, channel: Sink) {
        self.channel = Some(channel);
        let mut next = self.delivery.attach();
        while let Some(delivery) = next {
            next = self.deliver(delivery);
        }
    }

    fn deliver(&mut self, delivery: PtyOutputDelivery) -> Option<PtyOutputDelivery> {
        let Some(channel) = &self.channel else {
            self.delivery.delivery_failed(delivery);
            return None;
        };
        let kind = delivery.kind;
        if channel.send_output(delivery.bytes.clone()) {
            self.delivery.delivery_succeeded(kind)
        } else {
            self.channel = None;
            self.delivery.delivery_failed(delivery);
            None
        }
    }
}

pub struct PtyOutput<Sink, Logger> {
    state: Arc<Mutex<PtyOutputState<Sink, Logger>>>,
}

impl<Sink, Logger> Clone for PtyOutput<Sink, Logger> {
    fn clone(&self) -> Self {
        Self { state: Arc::clone(&self.state) }
    }
}

impl<Sink, Logger> PtyOutput<Sink, Logger> {
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(PtyOutputState::new())) }
    }

    pub fn new_with_logger(logger: Arc<Mutex<Logger>>) -> Self {
        Self { state: Arc::new(Mutex::new(PtyOutputState::new_with_logger(logger))) }
    }
}

impl<Sink, Logger> Default for PtyOutput<Sink, Logger> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Sink, Logger> PtyOutput<Sink, Logger>
where
    Sink: PtyOutputSink,
    Logger: PtyOutputLogger,
{
    pub fn send(&self, bytes: Vec<u8>) {
        self.state.lock().unwrap().send_or_buffer(bytes);
    }

    pub fn attach(&self, channel: Sink) {
        self.state.lock().unwrap().attach(channel);
    }
}

pub struct PtySession<Sink, Logger> {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
    pub writer: PtyWriter,
    pub output: PtyOutput<Sink, Logger>,
    pub generation: u64,
    pub ready_gate: Option<ReadyGate>,
    pub metadata: PtySessionMetadata,
    pub last_activity: Instant,
    pub logger: Option<Arc<Mutex<Logger>>>,
}

impl<Sink, Logger> PtySessionRegistryEntry for PtySession<Sink, Logger> {
    fn metadata(&self) -> &PtySessionMetadata {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut PtySessionMetadata {
        &mut self.metadata
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn touch_last_activity(&mut self) {
        self.last_activity = Instant::now();
    }
}

/// Placeholder for a child process that's already being waited on by another thread.
#[derive(Debug)]
pub struct WaitedChild;

impl portable_pty::ChildKiller for WaitedChild {
    fn kill(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(WaitedChild)
    }
}

impl portable_pty::Child for WaitedChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }

    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        Err(std::io::Error::other("child already waited"))
    }

    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<*mut std::ffi::c_void> {
        None
    }
}

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("Failed to open PTY: {source}")]
    OpenPty {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to spawn shell: {source}")]
    SpawnShell {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to spawn task: {source}")]
    SpawnTask {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to get PTY writer: {source}")]
    GetWriter {
        #[source]
        source: anyhow::Error,
    },
    #[error("Failed to get PTY reader: {source}")]
    GetReader {
        #[source]
        source: anyhow::Error,
    },
    #[error("Session {session_id} not found")]
    SessionNotFound { session_id: String },
    #[error("Write failed: {source}")]
    WriteFailed {
        #[source]
        source: std::io::Error,
    },
    #[error("Flush failed: {source}")]
    FlushFailed {
        #[source]
        source: std::io::Error,
    },
    #[error("Resize failed: {source}")]
    ResizeFailed {
        #[source]
        source: anyhow::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty_output::PTY_BACKLOG_LIMIT_BYTES;

    #[derive(Default)]
    struct TestLogger {
        bytes: Vec<u8>,
    }

    impl PtyOutputLogger for TestLogger {
        fn write_output(&mut self, bytes: &[u8]) {
            self.bytes.extend_from_slice(bytes);
        }

        fn recent_output(&self, max_bytes: usize) -> Vec<u8> {
            let start = self.bytes.len().saturating_sub(max_bytes);
            self.bytes[start..].to_vec()
        }
    }

    struct TestSink {
        received: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl PtyOutputSink for TestSink {
        fn send_output(&self, bytes: Vec<u8>) -> bool {
            self.received.lock().unwrap().push(bytes);
            true
        }
    }

    #[test]
    fn logger_receives_bytes_sent_to_output() {
        let logger = Arc::new(Mutex::new(TestLogger::default()));
        let output = PtyOutput::<TestSink, TestLogger>::new_with_logger(Arc::clone(&logger));

        output.send(vec![1, 2, 3]);
        output.send(vec![4, 5, 6]);

        assert_eq!(logger.lock().unwrap().recent_output(1024), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn buffers_output_until_channel_attaches() {
        let output = PtyOutput::<TestSink, TestLogger>::new();
        output.send(vec![1, 2, 3]);
        output.send(vec![4, 5, 6]);

        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(TestSink { received: Arc::clone(&received) });

        assert_eq!(*received.lock().unwrap(), vec![vec![1, 2, 3], vec![4, 5, 6]]);
    }

    #[test]
    fn trims_oldest_backlog_when_limit_is_exceeded() {
        let output = PtyOutput::<TestSink, TestLogger>::new();
        output.send(vec![1; PTY_BACKLOG_LIMIT_BYTES / 2]);
        output.send(vec![2; PTY_BACKLOG_LIMIT_BYTES / 2]);
        output.send(vec![3; PTY_BACKLOG_LIMIT_BYTES / 2]);

        let received = Arc::new(Mutex::new(Vec::new()));
        output.attach(TestSink { received: Arc::clone(&received) });

        assert_eq!(
            *received.lock().unwrap(),
            vec![vec![2; PTY_BACKLOG_LIMIT_BYTES / 2], vec![3; PTY_BACKLOG_LIMIT_BYTES / 2]]
        );
    }
}
