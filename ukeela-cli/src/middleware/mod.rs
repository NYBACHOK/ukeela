pub mod chain;
use crate::frame::Frame;

/// Process a frame - returns modified frame or error
pub type ProcessResult = Result<Frame, Box<dyn std::error::Error + Send + Sync>>;

#[enum_dispatch::enum_dispatch(MiddlewareDisplatch)]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &'static str;

    /// Process frame synchronously (for CPU path)
    fn process_sync(&self, frame: Frame) -> ProcessResult {
        // Default: run async in blocking context
        let rt = tokio::runtime::Handle::current();
        rt.block_on(self.process_async(frame))
    }

    /// Process frame asynchronously (for async pipelines)
    fn process_async(&self, frame: Frame) -> impl Future<Output = ProcessResult>;

    /// Check if middleware can use GPU acceleration
    fn has_gpu_support(&self) -> bool {
        false
    }

    /// Estimated processing latency in ms
    fn estimated_latency(&self) -> f32 {
        5.0 // 5ms default
    }
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
}

#[enum_dispatch::enum_dispatch]
#[derive(Debug)]
pub enum MiddlewareDisplatch {
    NoOp(NoOpMiddleware),
}
