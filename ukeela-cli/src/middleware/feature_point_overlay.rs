use crate::{
    frame::{Frame, VideoFormat},
    middleware::{Middleware, ProcessResult},
};

#[derive(Debug, Clone, Copy)]
pub struct FeaturePointOverlayMiddleware {
    frame_scale: f32,
}

impl FeaturePointOverlayMiddleware {
    pub fn new(frame_scale: f32) -> anyhow::Result<Self> {
        if !frame_scale.is_finite() || frame_scale <= 0.0 || frame_scale > 1.0 {
            return Err(anyhow::Error::msg(
                "Frame scale must be finite and in (0.0, 1.0]",
            ));
        }

        Ok(Self { frame_scale })
    }

    fn process_frame(&self, mut frame: Frame) -> ProcessResult {
        let Some(points) = frame.metadata.feature_points.as_ref() else {
            return Ok(frame);
        };
        if points.is_empty() {
            return Ok(frame);
        }

        let bytes_per_pixel = match &frame.format {
            VideoFormat::BGR => 3,
            VideoFormat::BGRA => 4,
        };

        let expected_len = usize::try_from(frame.width)
            .ok()
            .and_then(|width| width.checked_mul(usize::try_from(frame.height).ok()?))
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
            .ok_or_else(|| anyhow::anyhow!("Frame dimensions exceed addressable pixel data"))?;

        if frame.data.len() < expected_len {
            return Err(anyhow::anyhow!(
                "Frame data is too small for its dimensions and pixel format"
            )
            .into());
        }

        let width = usize::try_from(frame.width)?;
        let height = usize::try_from(frame.height)?;
        let scale = f64::from(self.frame_scale);
        let mut data = frame.data.to_vec();

        for &(feature_x, feature_y) in points {
            let x = (f64::from(feature_x) / scale).round();
            let y = (f64::from(feature_y) / scale).round();
            if x >= width as f64 || y >= height as f64 {
                continue;
            }

            let x = x as usize;
            let y = y as usize;
            for marker_y in y.saturating_sub(3)..=y.saturating_add(3).min(height - 1) {
                for marker_x in x.saturating_sub(3)..=x.saturating_add(3).min(width - 1) {
                    let dx = marker_x as isize - x as isize;
                    let dy = marker_y as isize - y as isize;
                    if dx * dx + dy * dy > 9 {
                        continue;
                    }

                    let offset = (marker_y * width + marker_x) * bytes_per_pixel;
                    data[offset] = 0;
                    data[offset + 1] = 255;
                    data[offset + 2] = 0;
                }
            }
        }

        frame.data = std::sync::Arc::new(data.into());

        Ok(frame)
    }
}

impl Middleware for FeaturePointOverlayMiddleware {
    fn name(&self) -> &'static str {
        "feature_point_overlay"
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(self.process_frame(frame))
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    fn frame(width: u32, height: u32, format: VideoFormat) -> Frame {
        let channels = match &format {
            VideoFormat::BGR => 3,
            VideoFormat::BGRA => 4,
        };
        Frame::new(
            Bytes::from(vec![10; width as usize * height as usize * channels]),
            width,
            height,
            format,
        )
    }

    #[tokio::test]
    async fn colors_scaled_bgr_feature_points_green() {
        let mut frame = frame(16, 16, VideoFormat::BGR);
        frame.metadata.feature_points = Some(vec![(4, 4), (50, 50)]);

        let frame = FeaturePointOverlayMiddleware::new(0.5)
            .unwrap()
            .process_async(frame)
            .await
            .unwrap();

        let pixel = (8 * 16 + 8) * 3;
        assert_eq!(&frame.data[pixel..pixel + 3], &[0, 255, 0]);
        assert_eq!(&frame.data[0..3], &[10, 10, 10]);
        let pixels_colored = frame
            .data
            .chunks_exact(3)
            .filter(|pixel| *pixel == [0, 255, 0])
            .count();
        assert_eq!(pixels_colored, 29);
    }

    #[tokio::test]
    async fn preserves_alpha_when_coloring_bgra_feature_points() {
        let mut frame = frame(2, 2, VideoFormat::BGRA);
        frame.metadata.feature_points = Some(vec![(1, 0)]);

        let frame = FeaturePointOverlayMiddleware::new(1.0)
            .unwrap()
            .process_async(frame)
            .await
            .unwrap();

        assert_eq!(&frame.data[4..8], &[0, 255, 0, 10]);
    }

    #[tokio::test]
    async fn leaves_frames_without_feature_points_unchanged() {
        let frame = frame(2, 2, VideoFormat::BGR);
        let original = frame.data.clone();

        let frame = FeaturePointOverlayMiddleware::new(1.0)
            .unwrap()
            .process_async(frame)
            .await
            .unwrap();

        assert_eq!(frame.data, original);
    }
}
