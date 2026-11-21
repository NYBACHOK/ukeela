use std::{
    fmt,
    num::NonZero,
    sync::{Arc, Mutex},
};

use image::GrayImage;
use optical_flow_lk::{
    DEFAULT_FB_THRESHOLD, DEFAULT_MIN_EIGEN_THRESHOLD, TrackParams, TrackStatus, TrackerContext,
    good_features_to_track,
};
use purecv::core::{KeyPoint, Matrix, Point2f};
use ukeela_common::{
    Frame, Middleware, MotionMatch, ProcessResult, extensions::lock::AcquireMutexLock,
};

pub mod feature_detection;
pub mod motion_tracking;

const PYRAMID_LEVELS: usize = 3;
const WINDOW_SIZE: usize = 21;
const MAX_ITERATIONS: usize = 30;

#[derive(Debug, Clone, Copy)]
pub struct FeatureDetectionConfig {
    pub num_features: NonZero<usize>,
    pub distance_to_border: u32,
}

impl Default for FeatureDetectionConfig {
    fn default() -> Self {
        Self {
            num_features: NonZero::new(100).expect("default is valid"),
            distance_to_border: 50,
        }
    }
}

fn gray_image_from_matrix(matrix: &Matrix<u8>) -> anyhow::Result<GrayImage> {
    let width = u32::try_from(matrix.cols)?;
    let height = u32::try_from(matrix.rows)?;
    GrayImage::from_raw(width, height, matrix.data.clone())
        .ok_or_else(|| anyhow::anyhow!("grayscale matrix dimensions do not match its data"))
}

fn point_is_in_bounds(x: f32, y: f32, (width, height): (u32, u32)) -> bool {
    x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 && x < width as f32 && y < height as f32
}

#[cfg(test)]
mod tests {
    use std::{num::NonZero, sync::Arc};

    use ukeela_common::VideoFormat;

    use crate::{
        feature_detection::OpticalFlowLkFeatureDetectionMiddleware,
        motion_tracking::OpticalFlowLkMatchesMiddleware,
    };

    use super::*;

    #[tokio::test]
    async fn detects_and_tracks_features_in_consecutive_frames() {
        let width = 96;
        let height = 96;
        let grayscale = Matrix::from_vec(
            height,
            width,
            1,
            (0..height)
                .flat_map(|y| {
                    (0..width)
                        .map(move |x| ((x * 73 + y * 151 + x * y * 17) ^ (x * 31 + y * 47)) as u8)
                })
                .collect(),
        );
        let detector = OpticalFlowLkFeatureDetectionMiddleware::new(FeatureDetectionConfig {
            num_features: NonZero::new(50).expect("positive feature count"),
            distance_to_border: 12,
        });
        let matcher = OpticalFlowLkMatchesMiddleware::new(1);

        let mut first_frame = frame_with_grayscale(width, height, grayscale.clone());
        first_frame = detector
            .process_async(first_frame)
            .await
            .expect("feature detection succeeds");
        assert!(!first_frame.metadata.feature_points.is_empty());
        first_frame = matcher
            .process_async(first_frame)
            .await
            .expect("first frame initializes tracker");
        assert!(first_frame.metadata.motion_matches.is_empty());

        let mut second_frame = frame_with_grayscale(width, height, grayscale);
        second_frame = detector
            .process_async(second_frame)
            .await
            .expect("feature detection succeeds");
        second_frame = matcher
            .process_async(second_frame)
            .await
            .expect("second frame is tracked");

        assert!(!second_frame.metadata.motion_matches.is_empty());
        assert!(second_frame.metadata.motion_matches.iter().all(|matched| {
            (matched.previous.x - matched.current.x).abs() < 0.1
                && (matched.previous.y - matched.current.y).abs() < 0.1
        }));
    }

    fn frame_with_grayscale(width: usize, height: usize, grayscale: Matrix<u8>) -> Frame {
        let mut frame = Frame::new(
            image::RgbaImage::new(width as u32, height as u32),
            width,
            height,
            VideoFormat::RGBA,
        );
        frame.metadata.grayscale = Some(Arc::new(grayscale));
        frame
    }
}
