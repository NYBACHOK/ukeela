use purecv::features2d::Orb;

use crate::{
    frame::Frame,
    middleware::{Middleware, ProcessResult, feature_detection::FeatureDetectionConfig},
};

#[derive(Debug)]
pub struct OrbFeatureDetectionMiddleware {
    config: FeatureDetectionConfig,
    detector: Orb,
}

impl OrbFeatureDetectionMiddleware {
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

        let mut detector = Orb::default();
        detector.set_nfeatures(config.num_features);

        Ok(Self { config, detector })
    }
}

impl Default for OrbFeatureDetectionMiddleware {
    fn default() -> Self {
        Self::new(FeatureDetectionConfig::default()).expect("default ORB config is valid")
    }
}

impl Middleware for OrbFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "purecv_orb_feature_detection"
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(Ok(frame))
    }
}
