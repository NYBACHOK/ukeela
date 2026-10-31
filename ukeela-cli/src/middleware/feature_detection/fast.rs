use purecv::features2d::{FastFeatureDetector, FastType};

use crate::{
    frame::Frame,
    middleware::{Middleware, ProcessResult, feature_detection::FeatureDetectionConfig},
};

#[derive(Debug)]
pub struct FastFeatureDetectionMiddleware {
    config: FeatureDetectionConfig,
    detector: FastFeatureDetector,
}

impl FastFeatureDetectionMiddleware {
    pub fn new(config: FeatureDetectionConfig) -> anyhow::Result<Self> {
        if !config.frame_scale.is_finite() || config.frame_scale <= 0.0 || config.frame_scale > 1.0
        {
            return Err(anyhow::Error::msg(
                "Frame scale must be finite and in (0.0, 1.0]",
            ));
        }
        if config.num_features == 0 {
            return Err(anyhow::Error::msg("Number of features must be positive"));
        }

        let detector = FastFeatureDetector::new(config.fast_threshold, true, FastType::Type9_16);

        Ok(Self { config, detector })
    }
}

impl Default for FastFeatureDetectionMiddleware {
    fn default() -> Self {
        Self::new(FeatureDetectionConfig::default()).expect("default FAST config is valid")
    }
}

impl Middleware for FastFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "purecv_fast_feature_detection"
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(Ok(frame))
    }
}
