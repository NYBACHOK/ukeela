use tokio::sync::{
    broadcast,
    mpsc::error::{SendError, TrySendError},
    watch,
};

use crate::frame::Frame;

pub(super) type IngressFrame = (u64, Frame);

/// Handle for feeding frames into the latest-frame ingress slot.
pub struct InputHandle {
    pub(super) tx: watch::Sender<IngressFrame>,
    pub(super) sequence: u64,
    pub(super) shutdown_rx: broadcast::Receiver<()>,
}

impl std::fmt::Debug for InputHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InputHandle")
            .finish_non_exhaustive()
    }
}

impl InputHandle {
    pub fn send_frame(
        &mut self,
        frame: Frame,
    ) -> impl Future<Output = Result<(), SendError<Frame>>> {
        async move {
            match self.send_frame_nonblocking(frame) {
                Ok(_) => Ok(()),
                Err(TrySendError::Closed(frame) | TrySendError::Full(frame)) => {
                    Err(SendError(frame))
                }
            }
        }
    }

    pub fn send_frame_nonblocking(
        &mut self,
        mut frame: Frame,
    ) -> Result<bool, TrySendError<Frame>> {
        if self.shutdown_rx.try_recv().is_ok() || self.tx.is_closed() {
            return Err(TrySendError::Closed(frame));
        }

        self.sequence += 1;

        frame.metadata.original_order = Some(self.sequence);
        frame.metadata.order = Some(self.sequence);

        self.tx.send_replace((self.sequence, frame));

        Ok(true)
    }
}
