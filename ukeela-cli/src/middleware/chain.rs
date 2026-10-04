use crate::{
    POISONED_LOCK_MSG,
    frame::Frame,
    middleware::{Middleware, MiddlewareDisplatch, ProcessResult},
};

#[derive(Debug, Default)]
pub struct ProcessingChain {
    middlewares: Vec<MiddlewareDisplatch>,
}

impl ProcessingChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn add_middleware(mut self, mw: MiddlewareDisplatch) -> Self {
        self.middlewares.push(mw);
        self
    }

    /// Process frame through all middlewares
    pub async fn process(&self, mut frame: Frame) -> ProcessResult {
        for mw in &self.middlewares {
            let mw_start = std::time::Instant::now();
            let frame_id = frame.id;
            let processed = mw.process_async(frame).await;
            let elapsed = mw_start.elapsed();
            let middleware_name = mw.name();

            frame = match processed {
                Ok(processed) => {
                    processed
                        .metadata
                        .write()
                        .expect(POISONED_LOCK_MSG)
                        .processing_time
                        .insert(middleware_name, elapsed);
                    processed
                }
                Err(error) => {
                    tracing::debug!(
                        frame_id,
                        middleware = middleware_name,
                        processing_time_ms = elapsed.as_secs_f64() * 1000.0,
                        failed = true,
                        "Middleware processing failed"
                    );
                    return Err(error);
                }
            };

            tracing::trace!(
                frame_id,
                middleware = middleware_name,
                processing_time_ms = elapsed.as_secs_f64() * 1000.0,
                failed = false,
                "Middleware processing completed"
            );
        }

        Ok(frame)
    }

    pub fn middleware_names(&self) -> Vec<&str> {
        self.middlewares.iter().map(|m| m.name()).collect()
    }
}
