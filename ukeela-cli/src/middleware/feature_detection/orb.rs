use purecv::features2d::Orb;

use crate::{
    frame::Frame,
    middleware::{Middleware, ProcessResult, feature_detection::FeatureDetectionConfig},
};

#[derive(Debug)]
pub struct OrbFeatureDetectionMiddleware {
    detector: Orb,
}

impl OrbFeatureDetectionMiddleware {
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let mut detector = Orb::default();
        detector.set_nfeatures(config.num_features.get());
        detector.set_edge_threshold(config.distance_to_border as i32);
        detector.set_fast_threshold(config.fast_threshold);

        Self { detector }
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        if frame.metadata.grayscale.is_none() {
            return Err(anyhow::anyhow!("frame metadata don't contains grayscale"));
        }

        let frame = tokio::runtime::Handle::current()
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
        "orb_feature_detection"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
    }
}
