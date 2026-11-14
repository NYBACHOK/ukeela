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
    pub fn new(config: FeatureDetectionConfig) -> Self {
        let detector = FastFeatureDetector::new(config.fast_threshold, true, FastType::Type9_16);

        Self { config, detector }
    }

    // TODO: use spawn_blocking?
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
                purecv::core::Size::new(width, height),
            )?)
        };

        let border_x = ((width as f32 * 0.1)
            .max(self.config.distance_to_border as f32 * f32::from(self.config.frame_scale)))
        .min(width as f32 / 2.0);
        let border_y = ((height as f32 * 0.1)
            .max(self.config.distance_to_border as f32 * f32::from(self.config.frame_scale)))
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
                .take(self.config.num_features.get())
                .map(|keypoint| (keypoint.pt.x.round() as u32, keypoint.pt.y.round() as u32))
                .collect(),
        );
        frame.metadata.motion_vectors = None;
        frame.metadata.motion_matches = None;

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
        let frame = self.process_frame(frame)?;

        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use purecv::core::Matrix;

    use super::FastFeatureDetectionMiddleware;
    use crate::{
        frame::{Frame, VideoFormat},
        middleware::{
            Middleware, feature_detection::FeatureDetectionConfig,
            feature_point_overlay::FeaturePointOverlayMiddleware,
        },
        types::frame_scale::FrameScale,
    };

    fn textured_image(size: usize) -> Arc<Matrix<u8>> {
        let mut image = Matrix::new(size, size, 1);
        let mut state = 0x1234_5678_u32;
        for y in 0..size {
            for x in 0..size {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                image.data[y * size + x] = (state >> 24) as u8;
            }
        }
        Arc::new(image)
    }

    #[tokio::test]
    async fn detected_points_are_drawn_by_the_overlay() {
        let size = 64;

        let image = image::ImageBuffer::from_raw(size, size, vec![0; 64 * 64 * 4]).unwrap();

        let frame_scale = FrameScale::try_new(0.5).unwrap();
        let detector = FastFeatureDetectionMiddleware::new(FeatureDetectionConfig {
            frame_scale,
            distance_to_border: 0,
            ..FeatureDetectionConfig::default()
        });
        let overlay = FeaturePointOverlayMiddleware::new(frame_scale);
        let mut frame = Frame::new(image, size as usize, size as usize, VideoFormat::BGR);
        frame.metadata.grayscale = Some(textured_image(size as usize));

        let frame = detector
            .process_async(frame)
            .await
            .expect("FAST detection should succeed");
        let points = frame
            .metadata
            .feature_points
            .as_ref()
            .expect("feature points should be set");
        assert!(!points.is_empty());
        assert!(points.iter().all(|&(x, y)| x < 32 && y < 32));

        let frame = overlay
            .process_async(frame)
            .await
            .expect("overlay should succeed");
        let green_pixels = frame
            .data
            .as_raw()
            .chunks_exact(3)
            .filter(|pixel| *pixel == [0, 255, 0])
            .count();
        assert!(green_pixels > 0);
    }
}
