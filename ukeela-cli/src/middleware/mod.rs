use crate::{
    frame::Frame,
    middleware::{
        feature_detection::{
            fast::FastFeatureDetectionMiddleware, orb::OrbFeatureDetectionMiddleware,
            overlay::FeaturePointOverlayMiddleware,
        },
        grayscale::GrayscaleMiddleware,
        motion::{
            estimation::MotionEstimationMiddleware,
            matches::MotionMatchesMiddleware,
            optical_flow_lk::{
                OpticalFlowLkFeatureDetectionMiddleware, OpticalFlowLkMatchesMiddleware,
            },
            overlay::MotionMatchesOverlayMiddleware,
        },
        resize::ResizeMiddleware,
        stabilization::StabilizationMiddleware,
    },
};

pub mod chain;
pub mod feature_detection;
pub mod grayscale;
pub mod motion;
pub mod queue;
pub mod resize;
pub mod stabilization;

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
}

#[derive(Debug)]
pub enum MiddlewareDisplatch {
    NoOp(NoOpMiddleware),
    Fast(FastFeatureDetectionMiddleware),
    Orb(OrbFeatureDetectionMiddleware),
    FeaturePointOverlay(FeaturePointOverlayMiddleware),
    Grayscale(GrayscaleMiddleware),
    Resize(ResizeMiddleware),
    MotionMatches(MotionMatchesMiddleware),
    OpticalFlowLkFeatureDetection(OpticalFlowLkFeatureDetectionMiddleware),
    OpticalFlowLkMatches(OpticalFlowLkMatchesMiddleware),
    MotionMatchesOverlay(MotionMatchesOverlayMiddleware),
    MotionEstimation(MotionEstimationMiddleware),
    Stabilization(StabilizationMiddleware),
}

impl Middleware for MiddlewareDisplatch {
    fn name(&self) -> &'static str {
        match self {
            Self::NoOp(v) => v.name(),
            Self::Fast(v) => v.name(),
            Self::Orb(v) => v.name(),
            Self::FeaturePointOverlay(v) => v.name(),
            Self::Grayscale(v) => v.name(),
            Self::Resize(v) => v.name(),
            Self::MotionMatches(v) => v.name(),
            Self::OpticalFlowLkFeatureDetection(v) => v.name(),
            Self::OpticalFlowLkMatches(v) => v.name(),
            Self::MotionMatchesOverlay(v) => v.name(),
            Self::MotionEstimation(v) => v.name(),
            Self::Stabilization(v) => v.name(),
        }
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        match self {
            Self::NoOp(v) => v.process_async(frame).await,
            Self::Fast(v) => v.process_async(frame).await,
            Self::Orb(v) => v.process_async(frame).await,
            Self::FeaturePointOverlay(v) => v.process_async(frame).await,
            Self::Grayscale(v) => v.process_async(frame).await,
            Self::Resize(v) => v.process_async(frame).await,
            Self::MotionMatches(v) => v.process_async(frame).await,
            Self::OpticalFlowLkFeatureDetection(v) => v.process_async(frame).await,
            Self::OpticalFlowLkMatches(v) => v.process_async(frame).await,
            Self::MotionMatchesOverlay(v) => v.process_async(frame).await,
            Self::MotionEstimation(v) => v.process_async(frame).await,
            Self::Stabilization(v) => v.process_async(frame).await,
        }
    }
}
