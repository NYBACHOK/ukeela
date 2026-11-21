use std::marker::PhantomData;

use tokio::sync::mpsc::{Receiver, Sender, error::TrySendError};
use ukeela_common::{Frame, Middleware, ProcessResult};

use crate::ProcessingChain;

#[derive(Debug)]
pub struct MultiProcessingChain<T> {
    sender: Sender<ProcessResult>,
    receiver: Receiver<ProcessResult>,
    _phantom: PhantomData<T>,
}

impl<T: Middleware + Send + Sync + 'static> ProcessingChain<T> for MultiProcessingChain<T> {
    fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        Self {
            sender,
            receiver,
            _phantom: PhantomData,
        }
    }

    fn push(&mut self, frame: Frame) -> Option<Frame> {
        let frame = self.sender.try_send(Ok(frame));

        match frame {
            Ok(_) => None,
            Err(err) => Some(err.into_inner().expect("we wrapped frame into erro")),
        }
    }

    async fn pop(&mut self) -> Option<ProcessResult> {
        self.receiver.recv().await
    }

    fn add_middleware(&mut self, middleware: T) {
        let (new_sender, mut receiver) = tokio::sync::mpsc::channel::<ProcessResult>(1);
        std::mem::swap(&mut receiver, &mut self.receiver);

        tokio::spawn({
            async move {
                loop {
                    let frame = match receiver.recv().await {
                        Some(Ok(v)) => v,
                        Some(Err(_)) => unreachable!("error variant should be dropped in previous"),
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

                    match new_sender.try_send(Ok(frame)) {
                        Ok(()) => {}
                        Err(TrySendError::Full(Ok(frame))) => {
                            tracing::warn!(
                                frame_id = frame.id,
                                middleware = middleware.name(),
                                "Dropping frame because the next processing stage is full"
                            );
                        }
                        Err(TrySendError::Full(Err(_))) => {
                            unreachable!("middleware output is wrapped in Ok")
                        }
                        Err(TrySendError::Closed(_)) => break,
                    }
                }
            }
        });
    }
}
