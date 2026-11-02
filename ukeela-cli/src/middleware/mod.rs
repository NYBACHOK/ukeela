use crate::{
    frame::Frame,
    middleware::{
        feature_detection::{
            fast::FastFeatureDetectionMiddleware, orb::OrbFeatureDetectionMiddleware,
        },
        grayscale::GrayscaleMiddleware,
    },
};

pub mod chain;
pub mod feature_detection;
pub mod feature_point_overlay;
pub mod grayscale;
pub mod warp;

/// Process a frame - returns modified frame or error
pub type ProcessResult = Result<Frame, Box<dyn std::error::Error + Send + Sync>>;

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

    // fn reset(&self) // TODO: is it make sense?
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

#[derive(Debug)]
pub enum MiddlewareDisplatch {
    NoOp(NoOpMiddleware),
    Fast(FastFeatureDetectionMiddleware),
    Orb(OrbFeatureDetectionMiddleware),
    FeaturePointOverlay(feature_point_overlay::FeaturePointOverlayMiddleware),
    Grayscale(GrayscaleMiddleware),
    Warp(warp::WarpMiddleware),
    // Ema(EmaMiddleware),
    // FeaturePoints(FeaturePointsMiddleware),
    // OpenCVFeatures(FeatureDetectionMiddleware),
    // Stabilization(StabilizationMiddleware),
}

impl Middleware for MiddlewareDisplatch {
    fn name(&self) -> &'static str {
        match self {
            Self::NoOp(v) => v.name(),
            Self::Fast(v) => v.name(),
            Self::Orb(v) => v.name(),
            Self::FeaturePointOverlay(v) => v.name(),
            Self::Grayscale(v) => v.name(),
            Self::Warp(v) => v.name(),
            // MiddlewareDisplatch::Ema(v) => v.name(),
            // MiddlewareDisplatch::FeaturePoints(v) => v.name(),
            // MiddlewareDisplatch::OpenCVFeatures(v) => v.name(),
            // MiddlewareDisplatch::Stabilization(v) => v.name(),
        }
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        match self {
            Self::NoOp(v) => v.process_async(frame).await,
            Self::Fast(v) => v.process_async(frame).await,
            Self::Orb(v) => v.process_async(frame).await,
            Self::FeaturePointOverlay(v) => v.process_async(frame).await,
            Self::Grayscale(v) => v.process_async(frame).await,
            Self::Warp(v) => v.process_async(frame).await,
            // MiddlewareDisplatch::Ema(v) => v.process_async(frame).await,
            // MiddlewareDisplatch::FeaturePoints(v) => v.process_async(frame).await,
            // MiddlewareDisplatch::OpenCVFeatures(v) => v.process_async(frame).await,
            // MiddlewareDisplatch::Stabilization(v) => v.process_async(frame).await,
        }
    }
}
