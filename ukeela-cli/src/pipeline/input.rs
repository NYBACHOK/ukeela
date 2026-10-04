use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use tokio::sync::{
    broadcast,
    mpsc::error::{SendError, TrySendError},
    watch,
};

use crate::{POISONED_LOCK_MSG, frame::Frame};

pub(super) type IngressFrame = (u64, Frame);

/// Handle for feeding frames into the latest-frame ingress slot.
#[derive(Clone)]
pub struct InputHandle {
    pub(super) tx: watch::Sender<Option<IngressFrame>>,
    pub(super) sequence: Arc<AtomicU64>,
    pub(super) closed: Arc<AtomicBool>,
    pub(super) shutdown_rx: Arc<broadcast::Receiver<()>>,
}

impl std::fmt::Debug for InputHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InputHandle")
            .field("closed", &self.closed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl InputHandle {
    pub fn send_frame(&self, frame: Frame) -> impl Future<Output = Result<(), SendError<Frame>>> {
        async move {
            match self.send_frame_nonblocking(frame) {
                Ok(_) => Ok(()),
                Err(TrySendError::Closed(frame) | TrySendError::Full(frame)) => {
                    Err(SendError(frame))
                }
            }
        }
    }

    pub fn send_frame_nonblocking(&self, frame: Frame) -> Result<bool, TrySendError<Frame>> {
        if self.closed.load(Ordering::Acquire) || self.tx.is_closed() {
            return Err(TrySendError::Closed(frame));
        }

        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut metadata = frame.metadata.write().expect(POISONED_LOCK_MSG);
            metadata.original_order = Some(sequence);
            metadata.order = Some(sequence);
            metadata.arrival_time = Some(std::time::Instant::now());
        }
        self.tx.send_replace(Some((sequence, frame)));
        Ok(true)
    }

    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_rx.resubscribe()
    }
}
