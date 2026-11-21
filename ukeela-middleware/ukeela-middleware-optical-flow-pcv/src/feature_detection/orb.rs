use purecv::features2d::Orb;
use ukeela_common::{Frame, Middleware, ProcessResult};

use crate::feature_detection::FeatureDetectionConfig;

#[derive(Debug, Clone)]
pub struct OrbFeatureDetectionMiddleware {
    detector: Orb,
}

impl OrbFeatureDetectionMiddleware {
    #[must_use]
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let mut detector = Orb::default();
        detector.set_nfeatures(config.num_features.get());
        detector.set_edge_threshold(config.distance_to_border as i32);
        detector.set_fast_threshold(config.fast_threshold);

        Self { detector }
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

        let points = self.detector.detect(grayscale)?;

        frame.metadata.feature_points = points;

        Ok(frame)
    }
}

impl Default for OrbFeatureDetectionMiddleware {
    fn default() -> Self {
        Self::new(FeatureDetectionConfig::default())
    }
}

impl Middleware for OrbFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "of_feature_detection_pcv_orb"
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
