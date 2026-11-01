use crate::{
    frame::Frame,
    middleware::{Middleware, MiddlewareDisplatch, ProcessResult},
};

#[derive(Debug, Default)]
pub struct ProcessingChain {
    middlewares: Vec<MiddlewareDisplatch>,
}

impl ProcessingChain {
    #[must_use]
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    #[must_use]
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
                    return Err(error);
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

        Ok(frame)
    }

    #[must_use]
    pub fn middleware_names(&self) -> Vec<&str> {
        self.middlewares
            .iter()
            .map(super::Middleware::name)
            .collect()
    }
}
