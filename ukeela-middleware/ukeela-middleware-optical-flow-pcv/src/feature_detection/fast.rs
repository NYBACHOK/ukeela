use purecv::features2d::{FastFeatureDetector, FastType};
use ukeela_common::{Frame, Middleware, ProcessResult};

use super::FeatureDetectionConfig;

#[derive(Debug, Clone)]
pub struct FastFeatureDetectionMiddleware {
    detector: FastFeatureDetector,
    max_features: usize,
}

impl FastFeatureDetectionMiddleware {
    #[must_use]
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let detector = FastFeatureDetector::new(config.fast_threshold, true, FastType::Type9_16);

        Self {
            detector,
            max_features: config.num_features.get(),
        }
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        if frame.metadata.grayscale.is_none() {
            return Err(anyhow::anyhow!("frame metadata don't contains grayscale"));
        }

        let grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .expect("we checked that it some");

        let mut points = self.detector.detect(grayscale)?;

        points.sort_by(|a, b| {
            b.response
                .partial_cmp(&a.response)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        points.truncate(self.max_features);

        frame.metadata.feature_points = points;

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
        "of_feature_detection_pcv_fast"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        tokio::runtime::Handle::current()
            .spawn_blocking({
                let this = self.clone();

                move || this.process_frame(frame)
            })
            .await?
            .map_err(Into::into)
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame)
    }
}
