use std::sync::Arc;

use purecv::{core::Size, features2d::Orb};

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
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let mut detector = Orb::default();
        detector.set_nfeatures(config.num_features.get());

        Self { config, detector }
    }

    // TODO: use spawn_blocking
    pub fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let width =
            ((frame.width as f32 * f32::from(self.config.frame_scale)).round() as usize).max(1);
        let height =
            ((frame.height as f32 * f32::from(self.config.frame_scale)).round() as usize).max(1);

        let grayscale = if let Some(grayscale) = frame.metadata.grayscale.take() {
            grayscale
        } else {
            return Err(anyhow::anyhow!("frame metadata don't contains grayscale"));
        };

        let resized = if grayscale.cols == width && grayscale.rows == height {
            grayscale
        } else {
            Arc::new(purecv::imgproc::resize(
                &grayscale,
                Size::new(width, height),
            )?)
        };

        let border_x = ((width as f32 * 0.1)
            .max(self.config.distance_to_border as f32 * f32::from(self.config.frame_scale)))
        .min(width as f32 / 2.0);
        let border_y = ((height as f32 * 0.1)
            .max(self.config.distance_to_border as f32 * f32::from(self.config.frame_scale)))
        .min(height as f32 / 2.0);

        let points: Vec<(u32, u32)> = self
            .detector
            .detect(&resized)?
            .into_iter()
            .filter(|keypoint| {
                keypoint.pt.x >= border_x
                    && keypoint.pt.y >= border_y
                    && keypoint.pt.x < width as f32 - border_x
                    && keypoint.pt.y < height as f32 - border_y
            })
            .map(|keypoint| (keypoint.pt.x.round() as u32, keypoint.pt.y.round() as u32))
            .collect();

        frame.metadata.feature_points = Some(points);
        frame.metadata.motion_vectors = None;
        frame.metadata.motion_matches = None;

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
        let frame = self.process_frame(frame)?;

        Ok(frame)
    }
}
