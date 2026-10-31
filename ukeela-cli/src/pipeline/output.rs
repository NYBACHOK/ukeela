use tokio::sync::mpsc::Receiver;

use crate::frame::Frame;

/// Handle for reading frames from pipeline (used by output thread)
pub struct OutputHandle {
    pub(super) rx: Receiver<Frame>,
}

impl OutputHandle {
    pub async fn receive_frame(&mut self) -> Option<Frame> {
        self.rx.recv().await
    }

    pub async fn recv_with_timeout(&mut self, timeout: std::time::Duration) -> Option<Frame> {
        tokio::time::timeout(timeout, self.receive_frame())
            .await
            .ok()
            .flatten()
    }
}
