use ukeela_common::{Frame, Middleware, ProcessResult};

use crate::ProcessingChain;

#[derive(Debug, Default)]
pub struct SingleProcessingChain<T> {
    middlewares: Vec<T>,
    pending_frame: Option<Frame>,
}

impl<T: Middleware> SingleProcessingChain<T> {
    #[must_use]
    pub fn middleware_names(&self) -> Vec<&str> {
        self.middlewares.iter().map(Middleware::name).collect()
    }
}

impl<T: Middleware + Send + Sync + 'static> ProcessingChain<T> for SingleProcessingChain<T> {
    fn new() -> Self {
        Self {
            middlewares: Vec::new(),
            pending_frame: None,
        }
    }

    fn push(&mut self, frame: Frame) -> Option<Frame> {
        let mut frame = Some(frame);
        std::mem::swap(&mut self.pending_frame, &mut frame);
        frame
    }

    async fn pop(&mut self) -> Option<ProcessResult> {
        let mut frame = self.pending_frame.take()?;

        for mw in &self.middlewares {
            let mw_start = std::time::Instant::now();
            let frame_id = frame.id;
            let processed = mw.process_async(frame).await;
            let elapsed = mw_start.elapsed();

            frame = match processed {
                Ok(mut processed) => {
                    processed
                        .metadata
                        .processing_time
                        .insert(mw.name(), elapsed);
                    processed
                }
                Err(error) => {
                    tracing::error!(
                        frame_id,
                        middleware = mw.name(),
                        processing_time_ms = elapsed.as_secs_f64() * 1000.0,
                        failed = true,
                        "Middleware processing failed"
                    );
                    return Some(Err(error));
                }
            };

            tracing::trace!(
                frame_id,
                middleware = mw.name(),
                processing_time_ms = elapsed.as_secs_f64() * 1000.0,
                failed = false,
                "Middleware processing completed"
            );
        }

        Some(Ok(frame))
    }

    fn add_middleware(&mut self, middleware: T) {
        self.middlewares.push(middleware);
    }
}
