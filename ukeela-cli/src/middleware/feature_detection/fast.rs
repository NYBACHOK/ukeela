use purecv::features2d::{FastFeatureDetector, FastType};

use crate::{
    frame::Frame,
    middleware::{Middleware, ProcessResult, feature_detection::FeatureDetectionConfig},
};

#[derive(Debug)]
pub struct FastFeatureDetectionMiddleware {
    detector: FastFeatureDetector,
    max_features: usize,
}

impl FastFeatureDetectionMiddleware {
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let detector = FastFeatureDetector::new(config.fast_threshold, true, FastType::Type9_16);

        Self {
            detector,
            max_features: config.num_features.get(),
        }
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        if frame.metadata.grayscale.is_none() {
            return Err(anyhow::anyhow!("frame metadata don't contains grayscale"));
        }

        let mut frame = tokio::runtime::Handle::current()
            .spawn_blocking({
                let detector = self.detector.clone();
                let max_features = self.max_features;

                move || {
                    let grayscale = frame
                        .metadata
                        .grayscale
                        .as_ref()
                        .expect("we checked that it some");

                    let mut points = detector.detect(grayscale)?;

                    points.sort_by(|a, b| {
                        b.response
                            .partial_cmp(&a.response)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });

                    points.truncate(max_features);

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
