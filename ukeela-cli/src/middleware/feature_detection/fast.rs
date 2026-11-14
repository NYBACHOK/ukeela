use purecv::features2d::{FastFeatureDetector, FastType};

use crate::{
    frame::Frame,
    middleware::{Middleware, ProcessResult, feature_detection::FeatureDetectionConfig},
};

#[derive(Debug)]
pub struct FastFeatureDetectionMiddleware {
    detector: FastFeatureDetector,
}

impl FastFeatureDetectionMiddleware {
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let detector = FastFeatureDetector::new(config.fast_threshold, true, FastType::Type9_16);

        Self { detector }
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        if frame.metadata.grayscale.is_none() {
            return Err(anyhow::anyhow!("frame metadata don't contains grayscale"));
        }

        let mut frame = tokio::runtime::Handle::current()
            .spawn_blocking({
                let detector = self.detector.clone();

                move || {
                    let grayscale = frame
                        .metadata
                        .grayscale
                        .as_ref()
                        .expect("we checked that it some");

                    let points = detector.detect(grayscale)?;

                    frame.metadata.feature_points = points;

                    Result::<_, anyhow::Error>::Ok(frame)
                }
            })
            .await??;

        frame.metadata.motion_vectors = Vec::new();
        frame.metadata.motion_matches = Vec::new();

        Ok(frame)
    }
}

impl Default for FastFeatureDetectionMiddleware {
    fn default() -> Self {
        Self::new(FeatureDetectionConfig::default())
    }
}

impl Middleware for FastFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "fast_feature_detection"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
    }
}
