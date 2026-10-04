use std::sync::Arc;

use tokio::sync::{
    broadcast,
    mpsc::{
        Sender,
        error::{SendError, TrySendError},
    },
};

use crate::frame::Frame;

/// Handle for feeding frames into pipeline (used by input thread)
#[derive(Debug, Clone)]
pub struct InputHandle {
    pub(super) tx: Sender<Frame>,
    pub(super) shutdown_rx: Arc<broadcast::Receiver<()>>,
}

impl InputHandle {
    pub fn send_frame(&self, frame: Frame) -> impl Future<Output = Result<(), SendError<Frame>>> {
        self.tx.send(frame)
    }

    pub fn send_frame_nonblocking(&self, frame: Frame) -> Result<bool, TrySendError<Frame>> {
        match self.tx.try_send(frame) {
            Ok(_) => Ok(true),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_rx.resubscribe()
    }
}
