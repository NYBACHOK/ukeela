use tokio::sync::mpsc::{Receiver, Sender, error::TrySendError};

use crate::{frame::Frame, middleware::Middleware};

#[derive(Debug)]
pub struct ProcessingChain {
    sender: Sender<Frame>,
    receiver: Receiver<Frame>,
}

impl ProcessingChain {
    pub fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        Self { sender, receiver }
    }

    pub fn push(&self, frame: Frame) -> Result<(), TrySendError<Frame>> {
        self.sender.try_send(frame)
    }

    pub async fn pop(&mut self) -> Option<Frame> {
        self.receiver.recv().await
    }

    pub fn add_middleware<T: Middleware + Send + Sync + 'static>(&mut self, middleware: T) {
        let (new_sender, mut receiver) = tokio::sync::mpsc::channel::<Frame>(1);
        std::mem::swap(&mut receiver, &mut self.receiver);

        let mut pending_frame = Option::<Frame>::None;

        tokio::spawn({
            async move {
                loop {
                    if let Some(frame) = pending_frame.take() {
                        let _ = new_sender.send(frame).await;
                    }

                    let frame = match receiver.recv().await {
                        Some(v) => v,
                        None => break,
                    };
                    let frame_id = frame.id;

                    let mw_start = std::time::Instant::now();

                    let frame_result = middleware.process_async(frame).await;
                    let elapsed = mw_start.elapsed();

                    let mut frame = match frame_result {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!(
                                frame_id,
                                middleware = middleware.name(),
                                processing_time_ms = elapsed.as_secs_f64() * 1000.0,
                                failed = true,
                                error = ?e,
                                "Middleware processing failed"
                            );
                            continue;
                        }
                    };

                    frame
                        .metadata
                        .processing_time
                        .insert(middleware.name(), elapsed);

                    if let Err(TrySendError::Full(frame)) = new_sender.try_send(frame) {
                        pending_frame = Some(frame)
                    }
                }
            }
        });
    }
}
