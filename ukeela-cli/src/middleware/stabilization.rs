use std::{
    io,
    sync::{Arc, Mutex},
};

use bytes::Bytes;

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, VideoFormat},
    middleware::{Middleware, ProcessResult},
};

const MAX_POINT_MATCH_DISTANCE: u32 = 64;

#[derive(Debug, Clone)]
pub struct StabilizationConfig {
    /// Total percentage cropped from each frame dimension, split across opposite edges.
    pub crop_percent: Option<u8>,
    /// Smoothing applied to the estimated camera trajectory.
    pub alpha: f32,
    /// Maximum camera movement accepted from one frame.
    pub max_delta: f32,
}

impl Default for StabilizationConfig {
    fn default() -> Self {
        Self {
            crop_percent: None,
            alpha: 0.15,
            max_delta: 50.0,
        }
    }
}

#[derive(Debug)]
struct StabilizationState {
    config: StabilizationConfig,
    cumulative_x: f32,
    cumulative_y: f32,
    smoothed_x: f32,
    smoothed_y: f32,
    previous_points: Option<Vec<(u32, u32)>>,
}

impl StabilizationState {
    fn new(config: StabilizationConfig) -> Self {
        Self {
            config,
            cumulative_x: 0.0,
            cumulative_y: 0.0,
            smoothed_x: 0.0,
            smoothed_y: 0.0,
            previous_points: None,
        }
    }

    fn update(&mut self, dx: f32, dy: f32) -> (f32, f32) {
        self.cumulative_x += dx.clamp(-self.config.max_delta, self.config.max_delta);
        self.cumulative_y += dy.clamp(-self.config.max_delta, self.config.max_delta);
        self.smoothed_x =
            self.config.alpha * self.cumulative_x + (1.0 - self.config.alpha) * self.smoothed_x;
        self.smoothed_y =
            self.config.alpha * self.cumulative_y + (1.0 - self.config.alpha) * self.smoothed_y;

        (
            self.smoothed_x - self.cumulative_x,
            self.smoothed_y - self.cumulative_y,
        )
    }
}

#[derive(Debug)]
pub struct StabilizationMiddleware {
    state: Arc<Mutex<StabilizationState>>,
}

impl StabilizationMiddleware {
    pub fn new(config: StabilizationConfig) -> Result<Self, &'static str> {
        if config
            .crop_percent
            .is_some_and(|percent| !(1..100).contains(&percent))
        {
            return Err("Crop percentage must be in [1, 100)");
        }
        if !(0.0..=1.0).contains(&config.alpha) {
            return Err("Alpha must be in [0.0, 1.0]");
        }
        if !config.max_delta.is_finite() || config.max_delta < 0.0 {
            return Err("Maximum delta must be finite and non-negative");
        }

        Ok(Self {
            state: Arc::new(Mutex::new(StabilizationState::new(config))),
        })
    }
}

impl Default for StabilizationMiddleware {
    fn default() -> Self {
        Self::new(StabilizationConfig::default()).expect("default stabilization config is valid")
    }
}

impl Middleware for StabilizationMiddleware {
    fn name(&self) -> &'static str {
        "stabilization"
    }

    fn estimated_latency(&self) -> f32 {
        3.0
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(process_frame(&self.state, frame))
    }
}

fn process_frame(state: &Mutex<StabilizationState>, frame: Frame) -> ProcessResult {
    let (motion_vectors, feature_points) = {
        let metadata = frame.metadata.read().expect(POISONED_LOCK_MSG);
        (
            metadata.motion_vectors.clone().filter(|v| !v.is_empty()),
            metadata.feature_points.clone().filter(|v| !v.is_empty()),
        )
    };

    if motion_vectors.is_none() && feature_points.is_none() {
        state.lock().expect(POISONED_LOCK_MSG).previous_points = None;
        return Ok(frame);
    }

    let mut state = state.lock().expect(POISONED_LOCK_MSG);
    let point_motion = feature_points.as_ref().and_then(|points| {
        state
            .previous_points
            .as_ref()
            .and_then(|previous| estimate_point_motion(previous, points))
    });
    state.previous_points = feature_points;
    let motion = if let Some(vectors) = motion_vectors {
        let count = vectors.len() as f32;
        Some((
            vectors.iter().map(|(dx, _)| f32::from(*dx)).sum::<f32>() / count,
            vectors.iter().map(|(_, dy)| f32::from(*dy)).sum::<f32>() / count,
        ))
    } else {
        point_motion
    };

    let Some((dx, dy)) = motion else {
        return Ok(frame);
    };

    let (correction_x, correction_y) = state.update(dx, dy);
    let crop_percent = state.config.crop_percent;
    drop(state);

    crop_and_translate(frame, correction_x, correction_y, crop_percent)
}

fn estimate_point_motion(previous: &[(u32, u32)], current: &[(u32, u32)]) -> Option<(f32, f32)> {
    let mut used = vec![false; current.len()];
    let mut displacements = Vec::with_capacity(previous.len().min(current.len()));

    for &(previous_x, previous_y) in previous {
        let nearest = current
            .iter()
            .enumerate()
            .filter(|(index, _)| !used[*index])
            .map(|(index, &(x, y))| {
                let dx = i64::from(x) - i64::from(previous_x);
                let dy = i64::from(y) - i64::from(previous_y);
                (index, x, y, dx * dx + dy * dy)
            })
            .min_by_key(|candidate| candidate.3);

        if let Some((index, x, y, distance_squared)) = nearest
            && distance_squared <= i64::from(MAX_POINT_MATCH_DISTANCE).pow(2)
        {
            used[index] = true;
            displacements.push((
                i64::from(x) - i64::from(previous_x),
                i64::from(y) - i64::from(previous_y),
            ));
        }
    }

    if displacements.is_empty() {
        return None;
    }

    displacements.sort_unstable_by_key(|motion| motion.0);
    let median_x = displacements[displacements.len() / 2].0;
    displacements.sort_unstable_by_key(|motion| motion.1);
    let median_y = displacements[displacements.len() / 2].1;
    Some((
        f32::from(i16::try_from(median_x).expect("matched point displacement is within 64px")),
        f32::from(i16::try_from(median_y).expect("matched point displacement is within 64px")),
    ))
}

fn crop_and_translate(
    mut frame: Frame,
    correction_x: f32,
    correction_y: f32,
    crop_percent: Option<u8>,
) -> ProcessResult {
    let width =
        usize::try_from(frame.width).map_err(|_| invalid_frame("frame width is too large"))?;
    let height =
        usize::try_from(frame.height).map_err(|_| invalid_frame("frame height is too large"))?;
    let crop_percent = crop_percent.unwrap_or(0);
    let mut crop_pixels_x = crop_for_dimension(width, crop_percent);
    let mut crop_pixels_y = crop_for_dimension(height, crop_percent);
    if matches!(frame.format, VideoFormat::I420 | VideoFormat::NV12) {
        crop_pixels_x = crop_pixels_x / 2 * 2;
        crop_pixels_y = crop_pixels_y / 2 * 2;
    }
    let crop_x =
        usize::try_from(crop_pixels_x).map_err(|_| invalid_frame("crop size is too large"))?;
    let crop_y =
        usize::try_from(crop_pixels_y).map_err(|_| invalid_frame("crop size is too large"))?;
    let twice_crop_x = crop_x
        .checked_mul(2)
        .ok_or_else(|| invalid_frame("crop size overflows frame dimensions"))?;
    let twice_crop_y = crop_y
        .checked_mul(2)
        .ok_or_else(|| invalid_frame("crop size overflows frame dimensions"))?;
    let output_width = width
        .checked_sub(twice_crop_x)
        .filter(|dimension| *dimension > 0)
        .ok_or_else(|| invalid_frame("crop removes the entire frame width"))?;
    let output_height = height
        .checked_sub(twice_crop_y)
        .filter(|dimension| *dimension > 0)
        .ok_or_else(|| invalid_frame("crop removes the entire frame height"))?;

    let (data, crop_x, crop_y, shift_x, shift_y) = match frame.format {
        VideoFormat::RGB | VideoFormat::RGBA | VideoFormat::BGRA => {
            let bytes_per_pixel = if matches!(frame.format, VideoFormat::RGB) {
                3
            } else {
                4
            };
            let packed_row_bytes = width
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
            let row_stride = packed_row_stride(frame.data.len(), packed_row_bytes, height)?;
            let data = crop_packed(
                &frame.data,
                PackedFrameLayout {
                    width,
                    height,
                    row_stride,
                    bytes_per_pixel,
                    crop_x,
                    crop_y,
                },
                (correction_x, correction_y),
            );
            (
                data,
                crop_pixels_x,
                crop_pixels_y,
                rounded_shift(correction_x, crop_pixels_x, frame.width),
                rounded_shift(correction_y, crop_pixels_y, frame.height),
            )
        }
        VideoFormat::I420 | VideoFormat::NV12 => {
            if !width.is_multiple_of(2)
                || !height.is_multiple_of(2)
                || !crop_pixels_x.is_multiple_of(2)
                || !crop_pixels_y.is_multiple_of(2)
            {
                return Err(Box::new(invalid_frame(
                    "I420 and NV12 dimensions and crop sizes must be even",
                )));
            }
            let luma_len = width
                .checked_mul(height)
                .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
            let chroma_len = luma_len / 4;
            let expected_len = if matches!(frame.format, VideoFormat::I420) {
                luma_len.checked_add(
                    chroma_len
                        .checked_mul(2)
                        .ok_or_else(|| invalid_frame("frame dimensions overflow"))?,
                )
            } else {
                luma_len.checked_add(luma_len / 2)
            }
            .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
            if frame.data.len() < expected_len {
                return Err(Box::new(invalid_frame(
                    "frame buffer is too small for its dimensions",
                )));
            }

            let shift_x = rounded_shift(correction_x, crop_pixels_x, frame.width) / 2 * 2;
            let shift_y = rounded_shift(correction_y, crop_pixels_y, frame.height) / 2 * 2;
            let luma = crop_plane(
                &frame.data[..luma_len],
                width,
                height,
                crop_x,
                crop_y,
                shift_x,
                shift_y,
            );
            let mut data = luma;
            if matches!(frame.format, VideoFormat::I420) {
                for plane_index in 0..2 {
                    let plane_start = luma_len + plane_index * chroma_len;
                    data.extend_from_slice(&crop_plane(
                        &frame.data[plane_start..plane_start + chroma_len],
                        width / 2,
                        height / 2,
                        crop_x / 2,
                        crop_y / 2,
                        shift_x / 2,
                        shift_y / 2,
                    ));
                }
            } else {
                data.extend_from_slice(&crop_plane(
                    &frame.data[luma_len..expected_len],
                    width,
                    height / 2,
                    crop_x,
                    crop_y / 2,
                    shift_x,
                    shift_y / 2,
                ));
            }

            (data, crop_pixels_x, crop_pixels_y, shift_x, shift_y)
        }
    };

    frame.data = Arc::new(Bytes::from(data));
    frame.width =
        u32::try_from(output_width).map_err(|_| invalid_frame("output width overflow"))?;
    frame.height =
        u32::try_from(output_height).map_err(|_| invalid_frame("output height overflow"))?;

    let mut metadata = frame.metadata.write().expect(POISONED_LOCK_MSG);
    if let Some(points) = metadata.feature_points.as_mut() {
        points.retain_mut(|(x, y)| {
            let translated_x = i64::from(*x) - i64::from(shift_x) - i64::from(crop_x);
            let translated_y = i64::from(*y) - i64::from(shift_y) - i64::from(crop_y);
            if translated_x < 0
                || translated_y < 0
                || translated_x >= i64::from(frame.width)
                || translated_y >= i64::from(frame.height)
            {
                return false;
            }
            *x = u32::try_from(translated_x).expect("translated coordinate is within frame bounds");
            *y = u32::try_from(translated_y).expect("translated coordinate is within frame bounds");
            true
        });
    }
    drop(metadata);

    Ok(frame)
}

fn rounded_shift(correction: f32, crop_pixels: u32, dimension: u32) -> i32 {
    let max_shift = if crop_pixels == 0 {
        i32::try_from(dimension).unwrap_or(i32::MAX)
    } else {
        i32::try_from(crop_pixels).unwrap_or(i32::MAX)
    };
    let translation = (-correction).round();
    if translation.is_finite() {
        (translation as i32).clamp(-max_shift, max_shift)
    } else {
        0
    }
}

fn crop_for_dimension(dimension: usize, crop_percent: u8) -> u32 {
    let crop = u128::try_from(dimension).unwrap_or(u128::MAX) * u128::from(crop_percent) / 200;
    u32::try_from(crop).unwrap_or(u32::MAX)
}

struct PackedFrameLayout {
    width: usize,
    height: usize,
    row_stride: usize,
    bytes_per_pixel: usize,
    crop_x: usize,
    crop_y: usize,
}

fn crop_packed(data: &[u8], layout: PackedFrameLayout, correction: (f32, f32)) -> Vec<u8> {
    let PackedFrameLayout {
        width,
        height,
        row_stride,
        bytes_per_pixel,
        crop_x,
        crop_y,
    } = layout;
    let output_width = width - 2 * crop_x;
    let output_height = height - 2 * crop_y;
    let shift_x = rounded_shift(
        correction.0,
        u32::try_from(crop_x).unwrap_or(u32::MAX),
        u32::try_from(width).unwrap_or(u32::MAX),
    ) as isize;
    let shift_y = rounded_shift(
        correction.1,
        u32::try_from(crop_y).unwrap_or(u32::MAX),
        u32::try_from(height).unwrap_or(u32::MAX),
    ) as isize;
    let mut output = Vec::with_capacity(output_width * output_height * bytes_per_pixel);

    for y in 0..output_height {
        let source_y = (y as isize + crop_y as isize + shift_y).clamp(0, height as isize - 1);
        for x in 0..output_width {
            let source_x = (x as isize + crop_x as isize + shift_x).clamp(0, width as isize - 1);
            let start = source_y as usize * row_stride + source_x as usize * bytes_per_pixel;
            output.extend_from_slice(&data[start..start + bytes_per_pixel]);
        }
    }

    output
}

fn crop_plane(
    data: &[u8],
    width: usize,
    height: usize,
    crop_x: usize,
    crop_y: usize,
    shift_x: i32,
    shift_y: i32,
) -> Vec<u8> {
    let output_width = width - 2 * crop_x;
    let output_height = height - 2 * crop_y;
    let mut output = Vec::with_capacity(output_width * output_height);

    for y in 0..output_height {
        let source_y =
            (y as i64 + crop_y as i64 + i64::from(shift_y)).clamp(0, height as i64 - 1) as usize;
        for x in 0..output_width {
            let source_x =
                (x as i64 + crop_x as i64 + i64::from(shift_x)).clamp(0, width as i64 - 1) as usize;
            output.push(data[source_y * width + source_x]);
        }
    }

    output
}

fn packed_row_stride(
    data_len: usize,
    packed_row_bytes: usize,
    height: usize,
) -> Result<usize, io::Error> {
    let packed_len = packed_row_bytes
        .checked_mul(height)
        .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
    if data_len < packed_len {
        return Err(invalid_frame(
            "frame buffer is too small for its dimensions",
        ));
    }

    if height > 0 && data_len.is_multiple_of(height) && data_len / height >= packed_row_bytes {
        Ok(data_len / height)
    } else {
        Ok(packed_row_bytes)
    }
}

fn invalid_frame(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;

    use super::StabilizationConfig;
    use crate::{
        frame::{Frame, VideoFormat},
        middleware::{Middleware, StabilizationMiddleware},
    };

    fn frame_with_metadata() -> Frame {
        let mut pixels = Vec::with_capacity(8 * 8 * 4);
        for _y in 0..8 {
            for x in 0..8 {
                pixels.extend_from_slice(&[
                    u8::try_from(x).expect("test pixel fits in u8"),
                    0,
                    0,
                    255,
                ]);
            }
        }
        Frame::new(Bytes::from(pixels), 8, 8, VideoFormat::BGRA)
    }

    #[tokio::test]
    async fn passes_through_when_motion_data_is_missing() {
        let middleware = StabilizationMiddleware::default();
        let frame = frame_with_metadata();
        let original_data = frame.data.clone();

        let processed = middleware
            .process_async(frame)
            .await
            .expect("frame should pass through");

        assert_eq!(processed.width, 8);
        assert_eq!(processed.height, 8);
        assert!(Arc::ptr_eq(&processed.data, &original_data));
    }

    #[tokio::test]
    async fn applies_motion_vectors_and_symmetric_crop() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            crop_percent: Some(25),
            alpha: 0.0,
            max_delta: 10.0,
        })
        .expect("config should be valid");
        let frame = frame_with_metadata();
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .motion_vectors = Some(vec![(2, 0)]);

        let processed = middleware
            .process_async(frame)
            .await
            .expect("frame should be stabilized");

        assert_eq!((processed.width, processed.height), (6, 6));
        assert_eq!(processed.data.len(), 6 * 6 * 4);
        assert_eq!(&processed.data[..4], &[2, 0, 0, 255]);
    }

    #[tokio::test]
    async fn uses_feature_points_from_consecutive_frames() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            crop_percent: None,
            alpha: 0.0,
            max_delta: 10.0,
        })
        .expect("config should be valid");

        for offset in [0, 1] {
            let frame = frame_with_metadata();
            frame
                .metadata
                .write()
                .expect("metadata lock should not be poisoned")
                .feature_points = Some(vec![(3 + offset, 3), (5 + offset, 5)]);
            let processed = middleware
                .process_async(frame)
                .await
                .expect("frame should be processed");
            assert_eq!((processed.width, processed.height), (8, 8));
            if offset == 1 {
                assert_eq!(&processed.data[..4], &[1, 0, 0, 255]);
            }
        }
    }

    #[tokio::test]
    async fn crops_yuv_planes_with_even_offsets() {
        for format in [VideoFormat::I420, VideoFormat::NV12] {
            let data = if matches!(format, VideoFormat::I420) {
                vec![128; 8 * 8 + 2 * 4 * 4]
            } else {
                vec![128; 8 * 8 + 8 * 8 / 2]
            };
            let frame = Frame::new(Bytes::from(data), 8, 8, format);
            frame
                .metadata
                .write()
                .expect("metadata lock should not be poisoned")
                .motion_vectors = Some(vec![(2, 2)]);
            let middleware = StabilizationMiddleware::new(StabilizationConfig {
                crop_percent: Some(50),
                alpha: 0.0,
                max_delta: 10.0,
            })
            .expect("config should be valid");

            let processed = middleware
                .process_async(frame)
                .await
                .expect("YUV frame should be processed");
            assert_eq!((processed.width, processed.height), (4, 4));
            assert_eq!(processed.data.len(), 4 * 4 + 4 * 4 / 2);
        }
    }

    #[test]
    fn validates_configured_crop_percentage() {
        for crop_percent in [Some(0), Some(100)] {
            assert!(
                StabilizationMiddleware::new(StabilizationConfig {
                    crop_percent,
                    ..StabilizationConfig::default()
                })
                .is_err()
            );
        }

        assert!(
            StabilizationMiddleware::new(StabilizationConfig {
                crop_percent: Some(1),
                ..StabilizationConfig::default()
            })
            .is_ok()
        );
        assert!(
            StabilizationMiddleware::new(StabilizationConfig {
                crop_percent: Some(99),
                ..StabilizationConfig::default()
            })
            .is_ok()
        );
    }
}
