use crate::Frame;

/// Process a frame - returns modified frame or error
pub type ProcessResult = Result<Frame, anyhow::Error>;

pub trait Middleware: Send + Sync {
    fn name(&self) -> &'static str;

    /// Process frame synchronously (for CPU path)
    fn process(&self, frame: Frame) -> ProcessResult;

    /// Process frame asynchronously (for async pipelines)
    fn process_async(&self, frame: Frame) -> impl Future<Output = ProcessResult> + Send + Sync;
}

/// No-op middleware for passthrough
#[derive(Debug)]
pub struct NoOpMiddleware;

impl Middleware for NoOpMiddleware {
    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        Ok(frame)
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        Ok(frame)
    }
}
