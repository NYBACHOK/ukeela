use std::{
    fmt,
    sync::{Arc, Mutex},
};

use image::GrayImage;
use optical_flow_lk::{
    DEFAULT_FB_THRESHOLD, DEFAULT_MIN_EIGEN_THRESHOLD, TrackParams, TrackStatus, TrackerContext,
    good_features_to_track,
};
use purecv::core::{KeyPoint, Matrix, Point2f};

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, MotionMatch},
    middleware::{Middleware, ProcessResult, feature_detection::FeatureDetectionConfig},
};

const PYRAMID_LEVELS: usize = 3;
const WINDOW_SIZE: usize = 21;
const MAX_ITERATIONS: usize = 30;

#[derive(Debug, Clone)]
pub struct OpticalFlowLkFeatureDetectionMiddleware {
    max_features: usize,
    distance_to_border: u32,
}

impl OpticalFlowLkFeatureDetectionMiddleware {
    #[must_use]
    pub fn new(config: FeatureDetectionConfig) -> Self {
        Self {
            max_features: config.num_features.get(),
            distance_to_border: config.distance_to_border,
        }
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .ok_or(anyhow::anyhow!("frame metadata doesn't contain grayscale"))?
            .clone();
        let max_features = self.max_features;
        let distance_to_border = self.distance_to_border;

        tokio::runtime::Handle::current()
            .spawn_blocking(move || {
                let grayscale = gray_image_from_matrix(&grayscale)?;
                let width = grayscale.width();
                let height = grayscale.height();
                let points = good_features_to_track(&grayscale, 0.01, 0)
                    .into_iter()
                    .filter(|corner| {
                        corner.x >= distance_to_border
                            && corner.y >= distance_to_border
                            && corner.x < width.saturating_sub(distance_to_border)
                            && corner.y < height.saturating_sub(distance_to_border)
                    })
                    .take(max_features)
                    .map(|corner| {
                        KeyPoint::new(
                            Point2f::new(corner.x as f32, corner.y as f32),
                            7.0,
                            -1.0,
                            corner.score,
                            0,
                            -1,
                        )
                    })
                    .collect();

                frame.metadata.feature_points = points;
                Ok::<_, anyhow::Error>(frame)
            })
            .await?
    }
}

impl Middleware for OpticalFlowLkFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "optical_flow_lk_feature_detection"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
    }
}

#[derive(Clone)]
pub struct OpticalFlowLkMatchesMiddleware {
    min_matches: usize,
    state: Arc<Mutex<TrackerState>>,
}

struct TrackerState {
    tracker: TrackerContext,
    previous_dimensions: Option<(u32, u32)>,
    previous_points: Vec<(f32, f32)>,
}

impl OpticalFlowLkMatchesMiddleware {
    #[must_use]
    pub fn new(min_matches: usize) -> Self {
        Self {
            min_matches,
            state: Arc::new(Mutex::new(TrackerState {
                tracker: TrackerContext::new(),
                previous_dimensions: None,
                previous_points: Vec::new(),
            })),
        }
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .ok_or(anyhow::anyhow!("frame metadata doesn't contain grayscale"))?
            .clone();
        let feature_points: Vec<_> = frame
            .metadata
            .feature_points
            .iter()
            .map(|point| (point.pt.x, point.pt.y))
            .collect();
        let min_matches = self.min_matches;

        let motion_matches = tokio::runtime::Handle::current()
            .spawn_blocking({
                let state = Arc::clone(&self.state);

                move || {
                    let grayscale = gray_image_from_matrix(&grayscale)?;
                    let dimensions = grayscale.dimensions();
                    let mut state = state.lock().expect(POISONED_LOCK_MSG);
                    state.tracker.advance(&grayscale, PYRAMID_LEVELS);
                    let previous_points = std::mem::take(&mut state.previous_points);

                    let matches = if state.previous_dimensions == Some(dimensions)
                        && previous_points.len() >= min_matches
                    {
                        let params = TrackParams {
                            window_size: WINDOW_SIZE,
                            max_iterations: MAX_ITERATIONS,
                            min_eigen_threshold: DEFAULT_MIN_EIGEN_THRESHOLD,
                        };

                        state
                            .tracker
                            .track_fb(&previous_points, None, params, DEFAULT_FB_THRESHOLD)
                            .iter()
                            .zip(&previous_points)
                            .filter_map(|(tracked, &(x, y))| {
                                let (current_x, current_y) = tracked.pos;
                                (tracked.status == TrackStatus::Tracked
                                    && point_is_in_bounds(current_x, current_y, dimensions))
                                .then_some(MotionMatch {
                                    previous: Point2f::new(x, y),
                                    current: Point2f::new(current_x, current_y),
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };

                    state.previous_dimensions = Some(dimensions);
                    state.previous_points = feature_points;
                    Ok::<_, anyhow::Error>(matches)
                }
            })
            .await??;

        frame.metadata.motion_matches = motion_matches;
        Ok(frame)
    }
}

impl fmt::Debug for OpticalFlowLkMatchesMiddleware {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpticalFlowLkMatchesMiddleware")
            .field("min_matches", &self.min_matches)
            .finish_non_exhaustive()
    }
}

impl Middleware for OpticalFlowLkMatchesMiddleware {
    fn name(&self) -> &'static str {
        "optical_flow_lk_matches"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
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

    use crate::{frame::VideoFormat, middleware::feature_detection::FeatureDetectionConfig};

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
            fast_threshold: 30,
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
