use std::sync::Arc;

use crate::{
    POISONED_LOCK_MSG,
    frame::Frame,
    middleware::{Middleware, MiddlewareDisplatch, ProcessResult},
};

#[derive(Debug, Clone, Default)]
pub struct ProcessingChain {
    middlewares: Vec<Arc<MiddlewareDisplatch>>,
}

impl ProcessingChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn add_middleware(mut self, mw: MiddlewareDisplatch) -> Self {
        self.middlewares.push(Arc::new(mw));
        self
    }

    /// Process frame through all middlewares
    pub async fn process(&self, mut frame: Frame) -> ProcessResult {
        let start = std::time::Instant::now();

        for mw in &self.middlewares {
            let mw_start = std::time::Instant::now();
            frame = mw.process_async(frame).await?;

            // Record processing time
            frame
                .metadata
                .write()
                .expect(POISONED_LOCK_MSG)
                .processing_time
                .insert(mw.name(), mw_start.elapsed());
        }

        tracing::debug!(
            "Frame {} processed in {:.1}ms through {} middlewares",
            frame.id,
            start.elapsed().as_secs_f32() * 1000.0,
            self.middlewares.len()
        );

        Ok(frame)
    }

    pub fn middleware_names(&self) -> Vec<&str> {
        self.middlewares.iter().map(|m| m.name()).collect()
    }
}
