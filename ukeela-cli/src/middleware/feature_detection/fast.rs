use std::sync::Arc;

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

    // TODO: use spawn_blocking
    pub fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let width = ((frame.width as f32 * self.config.frame_scale).round() as usize).max(1);
        let height = ((frame.height as f32 * self.config.frame_scale).round() as usize).max(1);

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
                purecv::core::Size::new(width, height),
            )?)
        };

        let border_x = ((width as f32 * 0.1)
            .max(self.config.distance_to_border as f32 * self.config.frame_scale))
        .min(width as f32 / 2.0);
        let border_y = ((height as f32 * 0.1)
            .max(self.config.distance_to_border as f32 * self.config.frame_scale))
        .min(height as f32 / 2.0);

        let mut keypoints = self.detector.detect(&resized)?;
        keypoints.retain(|keypoint| {
            keypoint.pt.x >= border_x
                && keypoint.pt.y >= border_y
                && keypoint.pt.x < width as f32 - border_x
                && keypoint.pt.y < height as f32 - border_y
        });
        keypoints.sort_by(|left, right| right.response.total_cmp(&left.response));

        frame.metadata.feature_points = Some(
            keypoints
                .into_iter()
                .take(self.config.num_features)
                .map(|keypoint| {
                    (
                        (keypoint.pt.x / self.config.frame_scale).round() as u32,
                        (keypoint.pt.y / self.config.frame_scale).round() as u32,
                    )
                })
                .collect(),
        );
        frame.metadata.motion_vectors = None;
        frame.metadata.motion_matches = None;

        Ok(frame)
    }
}

impl Default for FastFeatureDetectionMiddleware {
    fn default() -> Self {
        Self::new(FeatureDetectionConfig::default()).expect("default FAST config is valid")
    }
}

impl Middleware for FastFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "fast_feature_detection"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        let frame = self.process_frame(frame)?;

        Ok(frame)
    }
}
