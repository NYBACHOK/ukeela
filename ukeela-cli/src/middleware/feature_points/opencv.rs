use std::{
    io,
    sync::{
        Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

use opencv::{
    core::{self, Mat, Point2f, Size, TermCriteria, UMat, UMatUsageFlags, Vector},
    features2d::{FastFeatureDetector, FastFeatureDetector_DetectorType},
    highgui, imgproc,
    prelude::*,
    video,
};

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, VideoFormat},
    middleware::{Middleware, ProcessResult},
};

const GRID_SIZE: usize = 16;
const MIN_MATCHES: usize = 10;
const LK_WINDOW_SIZE: i32 = 21;
const LK_MAX_LEVEL: i32 = 3;
const MIN_FEATURE_DISTANCE: f32 = 4.0;

#[derive(Debug, Clone)]
pub struct FeatureConfig {
    pub frame_scale: f32,
    pub crop_percent: Option<u8>,
    pub border_crop: i32,
    pub num_features: i32,
    pub fast_threshold: f32,
    pub backward_threshold: f32,
    pub distance_to_border: i32,
    pub feature_interval: u64,
    pub debug_features: bool,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            frame_scale: 0.25,
            crop_percent: None,
            border_crop: 50,
            num_features: 200,
            fast_threshold: 30.0,
            backward_threshold: 2.0,
            distance_to_border: 50,
            feature_interval: 750_000_000,
            debug_features: false,
        }
    }
}

impl FeatureConfig {
    fn validate(&self) -> Result<(), &'static str> {
        if !self.frame_scale.is_finite() || self.frame_scale <= 0.0 || self.frame_scale > 1.0 {
            return Err("Frame scale must be finite and in (0.0, 1.0]");
        }
        if self.num_features <= 0 {
            return Err("Number of features must be positive");
        }
        if !self.fast_threshold.is_finite()
            || self.fast_threshold < 0.0
            || self.fast_threshold.round() > i32::MAX as f32
        {
            return Err("FAST threshold must be finite and non-negative");
        }
        if !self.backward_threshold.is_finite() || self.backward_threshold < 0.0 {
            return Err("Backward threshold must be finite and non-negative");
        }
        if self.distance_to_border < 0 {
            return Err("Distance to border must be non-negative");
        }
        if self.border_crop < 0 {
            return Err("Border crop must be non-negative");
        }
        if self
            .crop_percent
            .is_some_and(|percent| !(1..100).contains(&percent))
        {
            return Err("Crop percentage must be in [1, 100)");
        }
        Ok(())
    }
}

#[derive(Debug)]
struct TrackedFeature {
    id: u32,
    point: Point2f,
}

#[derive(Debug, Default)]
struct OpticalFlowTracker {
    previous_frame: Option<UMat>,
    features: Vec<TrackedFeature>,
    last_detection: Option<Duration>,
    next_id: u32,
}

#[derive(Debug)]
pub struct FeatureDetectionMiddleware {
    config: FeatureConfig,
    tracker: Mutex<OpticalFlowTracker>,
    debug_window: Option<FeatureDebugWindow>,
}

impl FeatureDetectionMiddleware {
    pub fn new(config: FeatureConfig) -> anyhow::Result<Self> {
        config.validate().map_err(anyhow::Error::msg)?;
        core::set_use_opencl(true)?;
        let debug_window = config
            .debug_features
            .then(FeatureDebugWindow::new)
            .transpose()?;
        Ok(Self {
            config,
            tracker: Mutex::new(OpticalFlowTracker::default()),
            debug_window,
        })
    }
}

impl Default for FeatureDetectionMiddleware {
    fn default() -> Self {
        Self::new(FeatureConfig::default()).expect("default feature config is valid")
    }
}

impl Middleware for FeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "opencv_feature_detection"
    }

    fn estimated_latency(&self) -> f32 {
        5.0
    }

    fn has_gpu_support(&self) -> bool {
        true
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(process_frame(
            &self.config,
            &self.tracker,
            self.debug_window.as_ref(),
            frame,
        ))
    }
}

#[derive(Debug)]
struct DebugFrame {
    pixels: Vec<u8>,
    width: i32,
    height: i32,
}

#[derive(Debug)]
struct FeatureDebugWindow {
    frames: SyncSender<DebugFrame>,
}

impl FeatureDebugWindow {
    fn new() -> io::Result<Self> {
        let (frames, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("feature-debug-window".to_owned())
            .spawn(move || run_debug_window(receiver))
            .map_err(|error| {
                io::Error::other(format!("failed to start feature debug window: {error}"))
            })?;
        Ok(Self { frames })
    }

    fn show(&self, frame: DebugFrame) -> io::Result<()> {
        match self.frames.try_send(frame) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err(io::Error::other(
                "feature debug window thread stopped unexpectedly",
            )),
        }
    }
}

fn run_debug_window(frames: mpsc::Receiver<DebugFrame>) {
    let window_name = "Detected features";
    let mut window_created = false;
    while let Ok(frame) = frames.recv() {
        let result = (|| -> opencv::Result<bool> {
            let mut image = Mat::new_rows_cols_with_default(
                frame.height,
                frame.width,
                core::CV_8UC3,
                core::Scalar::all(0.0),
            )?;
            image.data_bytes_mut()?.copy_from_slice(&frame.pixels);
            if !window_created {
                highgui::named_window(window_name, highgui::WINDOW_AUTOSIZE)?;
                window_created = true;
            }
            highgui::imshow(window_name, &image)?;
            Ok(highgui::wait_key(1)? == 27)
        })();

        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => {
                tracing::error!(%error, "Feature debug window failed");
                break;
            }
        }
    }

    if window_created && let Err(error) = highgui::destroy_window(window_name) {
        tracing::debug!(%error, "Failed to close feature debug window");
    }
}

fn process_frame(
    config: &FeatureConfig,
    tracker: &Mutex<OpticalFlowTracker>,
    debug_window: Option<&FeatureDebugWindow>,
    frame: Frame,
) -> ProcessResult {
    let gray = preprocess_frame(&frame, config.frame_scale)?;
    let scaled_width = gray.cols();
    let scaled_height = gray.rows();
    let mut gray_umat = UMat::new(UMatUsageFlags::USAGE_DEFAULT);
    gray.copy_to(&mut gray_umat)?;

    let mut tracker = tracker.lock().expect(POISONED_LOCK_MSG);
    let mut matched_displacements = Vec::new();
    let mut current_features = Vec::new();

    if let Some(previous_frame) = tracker.previous_frame.as_ref()
        && previous_frame.cols() == scaled_width
        && previous_frame.rows() == scaled_height
    {
        let previous_points = points_vector(&tracker.features);
        if !previous_points.is_empty() {
            let mut forward_points = Vector::<Point2f>::new();
            let mut forward_status = Vector::<u8>::new();
            let mut forward_error = Vector::<f32>::new();
            video::calc_optical_flow_pyr_lk(
                previous_frame,
                &gray_umat,
                &previous_points,
                &mut forward_points,
                &mut forward_status,
                &mut forward_error,
                Size::new(LK_WINDOW_SIZE, LK_WINDOW_SIZE),
                LK_MAX_LEVEL,
                TermCriteria::new(3, 30, 0.01)?,
                0,
                1e-4,
            )?;

            let mut backward_points = Vector::<Point2f>::new();
            let mut backward_status = Vector::<u8>::new();
            let mut backward_error = Vector::<f32>::new();
            if !forward_points.is_empty() {
                video::calc_optical_flow_pyr_lk(
                    &gray_umat,
                    previous_frame,
                    &forward_points,
                    &mut backward_points,
                    &mut backward_status,
                    &mut backward_error,
                    Size::new(LK_WINDOW_SIZE, LK_WINDOW_SIZE),
                    LK_MAX_LEVEL,
                    TermCriteria::new(3, 30, 0.01)?,
                    0,
                    1e-4,
                )?;
            }

            let border = border_margins(config, scaled_width, scaled_height);
            for index in 0..tracker.features.len().min(forward_points.len()) {
                if index >= backward_points.len()
                    || index >= backward_status.len()
                    || forward_status.get(index)? == 0
                    || backward_status.get(index)? == 0
                {
                    continue;
                }
                let old_point = tracker.features[index].point;
                let new_point = forward_points.get(index)?;
                let returned_point = backward_points.get(index)?;
                let backward_dx = old_point.x - returned_point.x;
                let backward_dy = old_point.y - returned_point.y;
                if backward_dx.hypot(backward_dy) > config.backward_threshold
                    || !is_inside_border(new_point, scaled_width, scaled_height, border)
                {
                    continue;
                }

                matched_displacements.push((new_point.x - old_point.x, new_point.y - old_point.y));
                current_features.push(TrackedFeature {
                    id: tracker.features[index].id,
                    point: new_point,
                });
            }
        }
    }

    let detection_due = tracker.last_detection.is_none_or(|last| {
        frame
            .timestamp
            .checked_sub(last)
            .is_none_or(|elapsed| elapsed.as_nanos() >= u128::from(config.feature_interval))
    });
    if detection_due || current_features.len() < config.num_features as usize {
        let detected = detect_features(&gray_umat, config, scaled_width, scaled_height)?;
        for point in grid_sparsify(
            detected,
            config.num_features as usize,
            scaled_width as usize,
            scaled_height as usize,
            frame.timestamp.as_nanos() as usize,
        ) {
            if current_features.len() >= config.num_features as usize {
                break;
            }
            if current_features.iter().all(|existing| {
                (existing.point.x - point.x).hypot(existing.point.y - point.y)
                    >= MIN_FEATURE_DISTANCE
            }) {
                let id = tracker.next_id;
                tracker.next_id = tracker
                    .next_id
                    .checked_add(1)
                    .ok_or_else(|| invalid_frame("feature ID space exhausted"))?;
                current_features.push(TrackedFeature { id, point });
            }
        }
        tracker.last_detection = Some(frame.timestamp);
    }

    if let Some(debug_window) = debug_window {
        let (crop_x, crop_y) = debug_crop_offsets(config, frame.width, frame.height);
        let debug_frame = render_features(&gray_umat, &current_features, crop_x, crop_y)?;
        debug_window.show(debug_frame)?;
    }

    let scale = config.frame_scale;
    let output_points = current_features
        .iter()
        .map(|feature| {
            (
                (feature.point.x / scale).round() as u32,
                (feature.point.y / scale).round() as u32,
            )
        })
        .collect();
    let (motion_vectors, motion_matches) = if matched_displacements.len() >= MIN_MATCHES {
        let vectors = matched_displacements
            .iter()
            .map(|(dx, dy)| Ok((motion_component(*dx)?, motion_component(*dy)?)))
            .collect::<Result<Vec<_>, io::Error>>()?;
        let matches = matched_displacements
            .iter()
            .zip(current_features.iter())
            .map(|((dx, dy), feature)| {
                let current = (feature.point.x, feature.point.y);
                ((current.0 - dx, current.1 - dy), current)
            })
            .collect();
        (Some(vectors), Some(matches))
    } else {
        (None, None)
    };

    tracker.features = current_features;
    tracker.previous_frame = Some(gray_umat);
    drop(tracker);

    let mut metadata = frame.metadata.write().expect(POISONED_LOCK_MSG);
    metadata.feature_points = Some(output_points);
    metadata.motion_vectors = motion_vectors;
    metadata.motion_matches = motion_matches;
    drop(metadata);

    Ok(frame)
}

fn debug_crop_offsets(config: &FeatureConfig, width: u32, height: u32) -> (i32, i32) {
    let (crop_x, crop_y) = if let Some(percent) = config.crop_percent {
        (
            u64::from(width) * u64::from(percent) / 200,
            u64::from(height) * u64::from(percent) / 200,
        )
    } else {
        let crop_x = u64::try_from(config.border_crop).unwrap_or_default();
        (crop_x, crop_x * u64::from(height) / u64::from(width.max(1)))
    };
    (
        (crop_x as f32 * config.frame_scale).round() as i32,
        (crop_y as f32 * config.frame_scale).round() as i32,
    )
}

fn render_features(
    image: &UMat,
    features: &[TrackedFeature],
    crop_x: i32,
    crop_y: i32,
) -> opencv::Result<DebugFrame> {
    let mut visualization = Mat::default();
    imgproc::cvt_color(image, &mut visualization, imgproc::COLOR_GRAY2BGR, 0)?;
    let crop_x = crop_x.clamp(0, visualization.cols().saturating_sub(1) / 2);
    let crop_y = crop_y.clamp(0, visualization.rows().saturating_sub(1) / 2);
    let mut cropped = Mat::roi(
        &visualization,
        core::Rect::new(
            crop_x,
            crop_y,
            visualization.cols() - 2 * crop_x,
            visualization.rows() - 2 * crop_y,
        ),
    )?
    .try_clone()?;
    for feature in features {
        let x = feature.point.x.round() as i32 - crop_x;
        let y = feature.point.y.round() as i32 - crop_y;
        if x < 0 || y < 0 || x >= cropped.cols() || y >= cropped.rows() {
            continue;
        }
        imgproc::circle(
            &mut cropped,
            core::Point::new(x, y),
            3,
            core::Scalar::new(0.0, 255.0, 0.0, 0.0),
            1,
            imgproc::LINE_AA,
            0,
        )?;
    }
    Ok(DebugFrame {
        pixels: cropped.data_bytes()?.to_vec(),
        width: cropped.cols(),
        height: cropped.rows(),
    })
}

fn preprocess_frame(
    frame: &Frame,
    scale: f32,
) -> Result<Mat, Box<dyn std::error::Error + Send + Sync>> {
    let width = i32::try_from(frame.width)
        .map_err(|_| invalid_frame("frame width exceeds OpenCV limits"))?;
    let height = i32::try_from(frame.height)
        .map_err(|_| invalid_frame("frame height exceeds OpenCV limits"))?;
    if width <= 0 || height <= 0 {
        return Err(invalid_frame("frame dimensions must be non-zero").into());
    }

    let channels = match frame.format {
        VideoFormat::I420 | VideoFormat::NV12 => 1,
        VideoFormat::RGB | VideoFormat::BGR => 3,
        VideoFormat::RGBA | VideoFormat::BGRA => 4,
    };
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(channels))
        .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
    let height_usize =
        usize::try_from(height).map_err(|_| invalid_frame("frame height is too large"))?;
    let required_len = row_bytes
        .checked_mul(height_usize)
        .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
    if frame.data.len() < required_len {
        return Err(invalid_frame("frame buffer is too small for its dimensions").into());
    }
    let source_stride = if matches!(frame.format, VideoFormat::I420 | VideoFormat::NV12) {
        row_bytes
    } else if height_usize > 0
        && frame.data.len().is_multiple_of(height_usize)
        && frame.data.len() / height_usize >= row_bytes
    {
        frame.data.len() / height_usize
    } else {
        row_bytes
    };

    let mat_type = match channels {
        1 => core::CV_8UC1,
        3 => core::CV_8UC3,
        4 => core::CV_8UC4,
        _ => unreachable!(),
    };
    let mut source =
        Mat::new_rows_cols_with_default(height, width, mat_type, core::Scalar::all(0.0))?;
    let source_bytes = source.data_bytes_mut()?;
    for row in 0..height_usize {
        let source_start = row * source_stride;
        let target_start = row * row_bytes;
        source_bytes[target_start..target_start + row_bytes]
            .copy_from_slice(&frame.data[source_start..source_start + row_bytes]);
    }

    let mut gray = Mat::default();
    match frame.format {
        VideoFormat::I420 | VideoFormat::NV12 => source.copy_to(&mut gray)?,
        VideoFormat::RGB => imgproc::cvt_color(&source, &mut gray, imgproc::COLOR_RGB2GRAY, 0)?,
        VideoFormat::BGR => imgproc::cvt_color(&source, &mut gray, imgproc::COLOR_BGR2GRAY, 0)?,
        VideoFormat::RGBA => imgproc::cvt_color(&source, &mut gray, imgproc::COLOR_RGBA2GRAY, 0)?,
        VideoFormat::BGRA => imgproc::cvt_color(&source, &mut gray, imgproc::COLOR_BGRA2GRAY, 0)?,
    }

    let scaled_width = ((width as f32 * scale).round() as i32).max(1);
    let scaled_height = ((height as f32 * scale).round() as i32).max(1);
    if scaled_width == width && scaled_height == height {
        return Ok(gray);
    }
    let mut resized = Mat::default();
    imgproc::resize(
        &gray,
        &mut resized,
        Size::new(scaled_width, scaled_height),
        0.0,
        0.0,
        imgproc::INTER_AREA,
    )?;
    Ok(resized)
}

fn detect_features(
    image: &UMat,
    config: &FeatureConfig,
    width: i32,
    height: i32,
) -> opencv::Result<Vec<(Point2f, f32)>> {
    let threshold = config.fast_threshold.round() as i32;
    let mut detector =
        FastFeatureDetector::create(threshold, true, FastFeatureDetector_DetectorType::TYPE_9_16)?;
    let mut keypoints = Vector::new();
    detector.detect(image, &mut keypoints, &core::no_array())?;
    let border = border_margins(config, width, height);
    Ok(keypoints
        .iter()
        .filter_map(|keypoint| {
            let point = keypoint.pt();
            is_inside_border(point, width, height, border).then_some((point, keypoint.response()))
        })
        .collect())
}

fn grid_sparsify(
    mut candidates: Vec<(Point2f, f32)>,
    max_features: usize,
    width: usize,
    height: usize,
    start_cell: usize,
) -> Vec<Point2f> {
    let mut cells: Vec<Vec<(Point2f, f32)>> =
        (0..GRID_SIZE * GRID_SIZE).map(|_| Vec::new()).collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let height = height.max(1);
    for candidate in candidates.drain(..) {
        let cell_x = ((candidate.0.x.max(0.0) as usize * GRID_SIZE) / width).min(GRID_SIZE - 1);
        let cell_y = ((candidate.0.y.max(0.0) as usize * GRID_SIZE) / height).min(GRID_SIZE - 1);
        cells[cell_y * GRID_SIZE + cell_x].push(candidate);
    }
    for cell in &mut cells {
        cell.sort_by(|left, right| right.1.total_cmp(&left.1));
    }

    let mut selected = Vec::with_capacity(max_features.min(256));
    let start = start_cell % cells.len();
    let max_per_cell = cells.iter().map(Vec::len).max().unwrap_or_default();
    for rank in 0..max_per_cell {
        for offset in 0..cells.len() {
            if let Some((point, _)) = cells[(start + offset) % cells.len()].get(rank) {
                selected.push(*point);
                if selected.len() == max_features {
                    return selected;
                }
            }
        }
    }
    selected
}

fn points_vector(features: &[TrackedFeature]) -> Vector<Point2f> {
    let mut points = Vector::<Point2f>::new();
    for feature in features {
        points.push(feature.point);
    }
    points
}

fn border_margins(config: &FeatureConfig, width: i32, height: i32) -> (f32, f32) {
    let configured_margin = config.distance_to_border as f32 * config.frame_scale;
    (
        (width as f32 * 0.1).max(configured_margin),
        (height as f32 * 0.1).max(configured_margin),
    )
}

fn is_inside_border(
    point: Point2f,
    width: i32,
    height: i32,
    (border_x, border_y): (f32, f32),
) -> bool {
    point.x >= border_x
        && point.y >= border_y
        && point.x < width as f32 - border_x
        && point.y < height as f32 - border_y
}

fn motion_component(value: f32) -> Result<i16, io::Error> {
    if !value.is_finite() {
        return Err(invalid_frame("motion estimate is not finite"));
    }
    let rounded = value.round();
    if rounded < i16::MIN as f32 || rounded > i16::MAX as f32 {
        return Err(invalid_frame("motion estimate exceeds metadata range"));
    }
    Ok(rounded as i16)
}

fn invalid_frame(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{FeatureConfig, FeatureDetectionMiddleware};
    use crate::{
        frame::{Frame, VideoFormat},
        middleware::Middleware,
    };

    fn textured_frame(shift_x: usize, shift_y: usize, timestamp: u64) -> Frame {
        let width = 320_usize;
        let height = 240_usize;
        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let source_x = x.checked_sub(shift_x);
                let source_y = y.checked_sub(shift_y);
                let value = source_x
                    .zip(source_y)
                    .map(|(source_x, source_y)| {
                        let mut value = ((source_x / 4) as u32).wrapping_mul(0x9e37_79b1)
                            ^ ((source_y / 4) as u32).wrapping_mul(0x85eb_ca77);
                        value ^= value >> 16;
                        value = value.wrapping_mul(0x7feb_352d);
                        value ^= value >> 15;
                        value as u8
                    })
                    .unwrap_or(0);
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let mut frame = Frame::new(
            Bytes::from(pixels),
            u32::try_from(width).expect("test width fits in u32"),
            u32::try_from(height).expect("test height fits in u32"),
            VideoFormat::BGRA,
        );
        frame.timestamp = std::time::Duration::from_nanos(timestamp);
        frame
    }

    #[test]
    fn rejects_invalid_configuration() {
        let config = FeatureConfig {
            frame_scale: 0.0,
            ..FeatureConfig::default()
        };
        assert!(FeatureDetectionMiddleware::new(config).is_err());
    }

    #[tokio::test]
    async fn tracks_translation_and_requires_enough_matches() {
        let middleware = FeatureDetectionMiddleware::new(FeatureConfig {
            frame_scale: 0.5,
            num_features: 120,
            fast_threshold: 10.0,
            distance_to_border: 0,
            feature_interval: u64::MAX,
            ..FeatureConfig::default()
        })
        .expect("test configuration should be valid");

        let first = middleware
            .process_async(textured_frame(0, 0, 1))
            .await
            .expect("first frame should be processed");
        let first_metadata = first
            .metadata
            .read()
            .expect("metadata lock should not be poisoned");
        assert!(
            first_metadata
                .feature_points
                .as_ref()
                .is_some_and(|points| points.len() >= 10)
        );
        assert!(first_metadata.motion_vectors.is_none());
        drop(first_metadata);

        let next = middleware
            .process_async(textured_frame(4, 4, 2))
            .await
            .expect("next frame should be processed");
        let metadata = next
            .metadata
            .read()
            .expect("metadata lock should not be poisoned");
        let (dx, dy) = metadata
            .motion_vectors
            .as_ref()
            .and_then(|vectors| vectors.first())
            .copied()
            .expect("at least ten consistent tracks should produce motion");
        assert!((i32::from(dx) - 2).abs() <= 1);
        assert!((i32::from(dy) - 2).abs() <= 1);
        assert!(
            metadata
                .motion_matches
                .as_ref()
                .is_some_and(|matches| matches.len() >= 10)
        );
    }
}
