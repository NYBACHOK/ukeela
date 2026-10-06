use std::{
    io,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use opencv::{
    calib3d,
    core::{self, Mat, Point2f, Rect, Scalar, Size, UMat, UMatUsageFlags, Vector},
    imgproc,
    prelude::*,
};

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, MotionMatch, VideoFormat},
    middleware::gpu_warp::GpuWarpContext,
    middleware::{Middleware, ProcessResult},
};

const MIN_MATCHES: usize = 10;
const FIRST_RAW_FRAMES: u64 = 2;

#[derive(Debug, Clone)]
pub struct StabilizationConfig {
    /// Total percentage removed from each dimension, split across opposing edges.
    pub crop_percent: Option<u8>,
    /// Kalman process noise.
    pub q_scale: f64,
    /// Kalman measurement noise.
    pub r_scale: f64,
    /// Horizontal border crop in original-resolution pixels.
    pub border_crop: i32,
    /// Must match the feature detection middleware's frame scale.
    pub frame_scale: f32,
}

impl Default for StabilizationConfig {
    fn default() -> Self {
        Self {
            crop_percent: None,
            q_scale: 0.004,
            r_scale: 0.5,
            border_crop: 50,
            frame_scale: 0.25,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Measurement {
    dx: f64,
    dy: f64,
    da: f64,
    scale: f64,
}

impl Measurement {
    fn values(self) -> [f64; 5] {
        [self.dx, self.dy, self.da, self.scale, self.scale]
    }
}

#[derive(Debug, Clone, Copy)]
struct Smoothened {
    dx: f64,
    dy: f64,
    da: f64,
    scale_x: f64,
    scale_y: f64,
}

impl From<Measurement> for Smoothened {
    fn from(motion: Measurement) -> Self {
        Self {
            dx: motion.dx,
            dy: motion.dy,
            da: motion.da,
            scale_x: motion.scale,
            scale_y: motion.scale,
        }
    }
}

#[derive(Debug)]
struct KalmanState {
    estimate: [f64; 5],
    error: [f64; 5],
    sum: [f64; 5],
    q_scale: f64,
    r_scale: f64,
}

impl KalmanState {
    fn new(config: &StabilizationConfig) -> Self {
        Self {
            estimate: [0.0; 5],
            error: [1.0; 5],
            sum: [0.0; 5],
            q_scale: config.q_scale,
            r_scale: config.r_scale,
        }
    }

    fn seed(&mut self, measured: Measurement) {
        self.estimate = measured.values();
        for (sum, value) in self.sum.iter_mut().zip(measured.values()) {
            *sum += value;
        }
    }

    fn update(&mut self, measured: Measurement) -> Smoothened {
        for (index, value) in measured.values().into_iter().enumerate() {
            self.sum[index] += value;
            self.error[index] += self.q_scale;
            let gain = self.error[index] / (self.error[index] + self.r_scale);
            self.estimate[index] += gain * (value - self.estimate[index]);
            self.error[index] *= 1.0 - gain;
        }

        Smoothened {
            dx: self.estimate[0],
            dy: self.estimate[1],
            da: self.estimate[2],
            scale_x: self.estimate[3],
            scale_y: self.estimate[4],
        }
    }
}

#[derive(Debug)]
struct StabilizationState {
    kalman: KalmanState,
    frame_counter: u64,
    warped_frame: Option<UMat>,
}

impl StabilizationState {
    fn new(config: &StabilizationConfig) -> Self {
        Self {
            kalman: KalmanState::new(config),
            frame_counter: 0,
            warped_frame: None,
        }
    }
}

#[derive(Debug)]
pub struct StabilizationMiddleware {
    config: StabilizationConfig,
    state: Mutex<StabilizationState>,
    gpu: tokio::sync::OnceCell<Option<Arc<GpuWarpContext>>>,
}

impl StabilizationMiddleware {
    pub fn new(config: StabilizationConfig) -> Result<Self, &'static str> {
        if config
            .crop_percent
            .is_some_and(|percent| !(1..100).contains(&percent))
        {
            return Err("Crop percentage must be in [1, 100)");
        }
        if !config.q_scale.is_finite() || config.q_scale < 0.0 {
            return Err("Kalman process noise must be finite and non-negative");
        }
        if !config.r_scale.is_finite() || config.r_scale <= 0.0 {
            return Err("Kalman measurement noise must be finite and positive");
        }
        if config.border_crop < 0 {
            return Err("Border crop must be non-negative");
        }
        if !config.frame_scale.is_finite() || config.frame_scale <= 0.0 || config.frame_scale > 1.0
        {
            return Err("Frame scale must be finite and in (0.0, 1.0]");
        }

        Ok(Self {
            state: Mutex::new(StabilizationState::new(&config)),
            config,
            gpu: tokio::sync::OnceCell::new(),
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
        8.0
    }

    fn has_gpu_support(&self) -> bool {
        true
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        process_frame(&self.config, &self.state, &self.gpu, frame).await
    }
}

async fn process_frame(
    config: &StabilizationConfig,
    state_mutex: &Mutex<StabilizationState>,
    gpu_cell: &tokio::sync::OnceCell<Option<Arc<GpuWarpContext>>>,
    mut frame: Frame,
) -> ProcessResult {
    let (motion_matches, motion_vectors) = {
        let metadata = frame.metadata.read().expect(POISONED_LOCK_MSG);
        (
            metadata.motion_matches.clone(),
            metadata.motion_vectors.clone(),
        )
    };
    let measurement = if let Some(matches) = motion_matches
        .as_deref()
        .filter(|matches| matches.len() >= MIN_MATCHES)
    {
        estimate_motion(matches, config.frame_scale)?
    } else {
        None
    }
    .or_else(|| {
        motion_vectors
            .as_deref()
            .filter(|vectors| vectors.len() >= MIN_MATCHES)
            .and_then(|vectors| estimate_translation(vectors, config.frame_scale))
    });
    let Some(measurement) = measurement else {
        let mut state = state_mutex.lock().expect(POISONED_LOCK_MSG);
        state.frame_counter = state.frame_counter.saturating_add(1);
        return Ok(frame);
    };

    let motion = {
        let mut state = state_mutex.lock().expect(POISONED_LOCK_MSG);
        let motion = if state.frame_counter < FIRST_RAW_FRAMES {
            state.kalman.seed(measurement);
            Smoothened::from(measurement)
        } else {
            state.kalman.update(measurement)
        };
        state.frame_counter = state.frame_counter.saturating_add(1);
        motion
    };

    let inverse = inverse_transform(motion);
    let gpu = gpu_cell
        .get_or_init(|| async {
            match GpuWarpContext::new(true).await {
                Ok(context) => {
                    let capabilities = context.capabilities();
                    tracing::info!(
                        backend = ?capabilities.backend,
                        adapter = %capabilities.name,
                        max_texture_size = capabilities.max_texture_size,
                        "GPU affine warp initialized"
                    );
                    Some(Arc::new(context))
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "GPU affine warp unavailable; using OpenCV CPU warp"
                    );
                    None
                }
            }
        })
        .await;
    let direct_bgra = tightly_packed_bgra(&frame);
    let input_image = if direct_bgra.is_none() {
        Some(frame_to_bgra(&frame)?)
    } else {
        None
    };
    let input_pixels = match (direct_bgra, input_image.as_ref()) {
        (Some(pixels), _) => pixels,
        (None, Some(image)) => image.data_bytes()?,
        (None, None) => return Err(invalid_frame("Could not prepare BGRA frame data").into()),
    };
    let affine = [
        *inverse.at_2d::<f64>(0, 0)?,
        *inverse.at_2d::<f64>(0, 1)?,
        *inverse.at_2d::<f64>(0, 2)?,
        *inverse.at_2d::<f64>(1, 0)?,
        *inverse.at_2d::<f64>(1, 1)?,
        *inverse.at_2d::<f64>(1, 2)?,
    ];
    let determinant = affine[0] * affine[4] - affine[1] * affine[3];
    // OpenCV treats the supplied affine matrix as a forward transform and inverts it for sampling.
    let transform = [
        (affine[4] / determinant) as f32,
        (-affine[1] / determinant) as f32,
        ((affine[1] * affine[5] - affine[4] * affine[2]) / determinant) as f32,
        (-affine[3] / determinant) as f32,
        (affine[0] / determinant) as f32,
        ((affine[3] * affine[2] - affine[0] * affine[5]) / determinant) as f32,
    ];
    let width = frame.width;
    let height = frame.height;
    let warped = if let Some(gpu) = gpu.as_deref() {
        match gpu.warp_bgra(input_pixels, width, height, transform) {
            Ok(pixels) => bgra_from_bytes(&pixels, height)?,
            Err(error) => {
                tracing::warn!(%error, "GPU affine warp failed; using OpenCV CPU warp");
                let image = match input_image {
                    Some(image) => image,
                    None => frame_to_bgra(&frame)?,
                };
                warp_bgra_cpu(&image, &inverse, width, height, state_mutex)?
            }
        }
    } else {
        let image = match input_image {
            Some(image) => image,
            None => frame_to_bgra(&frame)?,
        };
        warp_bgra_cpu(&image, &inverse, width, height, state_mutex)?
    };
    let (crop_x, crop_y) = crop_borders(config, &frame)?;
    let cropped = Mat::roi(
        &warped,
        Rect::new(
            crop_x,
            crop_y,
            warped.cols() - 2 * crop_x,
            warped.rows() - 2 * crop_y,
        ),
    )?;
    let cropped = cropped.try_clone()?;
    let output = bgra_to_format(&cropped, frame.format.clone())?;
    frame.data = Bytes::from(output).into();
    frame.width = u32::try_from(cropped.cols())
        .map_err(|_| invalid_frame("output width exceeds frame limits"))?;
    frame.height = u32::try_from(cropped.rows())
        .map_err(|_| invalid_frame("output height exceeds frame limits"))?;
    let mut metadata = frame.metadata.write().expect(POISONED_LOCK_MSG);
    metadata.estimated_transform = Some([
        affine[0] as f32,
        affine[1] as f32,
        (affine[2] - f64::from(crop_x)) as f32,
        affine[3] as f32,
        affine[4] as f32,
        (affine[5] - f64::from(crop_y)) as f32,
    ]);
    if let Some(points) = metadata.feature_points.as_mut() {
        points.retain_mut(|(x, y)| {
            let transformed_x = affine[0] * f64::from(*x) + affine[1] * f64::from(*y) + affine[2]
                - f64::from(crop_x);
            let transformed_y = affine[3] * f64::from(*x) + affine[4] * f64::from(*y) + affine[5]
                - f64::from(crop_y);
            if !transformed_x.is_finite()
                || !transformed_y.is_finite()
                || transformed_x < 0.0
                || transformed_y < 0.0
                || transformed_x >= f64::from(frame.width)
                || transformed_y >= f64::from(frame.height)
            {
                return false;
            }
            *x = transformed_x.round() as u32;
            *y = transformed_y.round() as u32;
            true
        });
    }
    drop(metadata);
    Ok(frame)
}

fn warp_bgra_cpu(
    image: &Mat,
    inverse: &Mat,
    width: u32,
    height: u32,
    state_mutex: &Mutex<StabilizationState>,
) -> Result<Mat, Box<dyn std::error::Error + Send + Sync>> {
    let mut source_umat = UMat::new(UMatUsageFlags::USAGE_DEFAULT);
    image.copy_to(&mut source_umat)?;
    let mut state = state_mutex.lock().expect(POISONED_LOCK_MSG);
    let mut warped_umat = state
        .warped_frame
        .take()
        .unwrap_or_else(|| UMat::new(UMatUsageFlags::USAGE_DEFAULT));
    imgproc::warp_affine(
        &source_umat,
        &mut warped_umat,
        inverse,
        Size::new(
            i32::try_from(width).map_err(|_| invalid_frame("frame width exceeds OpenCV limits"))?,
            i32::try_from(height)
                .map_err(|_| invalid_frame("frame height exceeds OpenCV limits"))?,
        ),
        imgproc::INTER_LINEAR,
        core::BORDER_REFLECT,
        Scalar::default(),
    )?;

    let mut warped = Mat::default();
    warped_umat.copy_to(&mut warped)?;
    state.warped_frame = Some(warped_umat);
    Ok(warped)
}

fn bgra_from_bytes(
    pixels: &[u8],
    height: u32,
) -> Result<Mat, Box<dyn std::error::Error + Send + Sync>> {
    let pixels = Mat::from_slice(pixels)?;
    Ok(pixels.reshape(4, i32::try_from(height)?)?.try_clone()?)
}

fn tightly_packed_bgra(frame: &Frame) -> Option<&[u8]> {
    if !matches!(frame.format, VideoFormat::BGRA) {
        return None;
    }

    let required_len = usize::try_from(frame.width)
        .ok()?
        .checked_mul(usize::try_from(frame.height).ok()?)?
        .checked_mul(4)?;
    (frame.data.len() == required_len).then_some(&frame.data)
}

fn estimate_motion(
    matches: &[MotionMatch],
    frame_scale: f32,
) -> opencv::Result<Option<Measurement>> {
    let mut previous = Vector::<Point2f>::new();
    let mut current = Vector::<Point2f>::new();
    for &((previous_x, previous_y), (current_x, current_y)) in matches {
        if [previous_x, previous_y, current_x, current_y]
            .into_iter()
            .all(f32::is_finite)
        {
            previous.push(Point2f::new(previous_x, previous_y));
            current.push(Point2f::new(current_x, current_y));
        }
    }
    if previous.len() < MIN_MATCHES {
        return Ok(None);
    }

    let mut inliers = Mat::default();
    let affine = calib3d::estimate_affine_partial_2d(
        &previous,
        &current,
        &mut inliers,
        calib3d::RANSAC,
        3.0,
        2_000,
        0.99,
        10,
    )?;
    if affine.empty() || core::count_non_zero(&inliers)? < MIN_MATCHES as i32 {
        return Ok(None);
    }
    let a = *affine.at_2d::<f64>(0, 0)?;
    let b = *affine.at_2d::<f64>(1, 0)?;
    let translation_scale = f64::from(frame_scale);
    Ok(Some(Measurement {
        dx: *affine.at_2d::<f64>(0, 2)? / translation_scale,
        dy: *affine.at_2d::<f64>(1, 2)? / translation_scale,
        da: b.atan2(a),
        scale: a.hypot(b) - 1.0,
    }))
}

fn estimate_translation(vectors: &[(i16, i16)], frame_scale: f32) -> Option<Measurement> {
    let count = vectors.len() as f64;
    let frame_scale = f64::from(frame_scale);
    Some(Measurement {
        dx: vectors.iter().map(|(dx, _)| f64::from(*dx)).sum::<f64>() / count / frame_scale,
        dy: vectors.iter().map(|(_, dy)| f64::from(*dy)).sum::<f64>() / count / frame_scale,
        da: 0.0,
        scale: 0.0,
    })
}

fn inverse_transform(motion: Smoothened) -> Mat {
    let angle = motion.da;
    let scale = (1.0 + (motion.scale_x + motion.scale_y) / 2.0).max(f64::EPSILON);
    let a = scale * angle.cos();
    let b = scale * angle.sin();
    let determinant = a * a + b * b;
    let inverse_a = a / determinant;
    let inverse_b = b / determinant;
    let dx = -inverse_a * motion.dx - inverse_b * motion.dy;
    let dy = inverse_b * motion.dx - inverse_a * motion.dy;

    Mat::from_slice_2d(&[[inverse_a, inverse_b, dx], [-inverse_b, inverse_a, dy]])
        .expect("affine matrix has a fixed valid shape")
}

fn crop_borders(config: &StabilizationConfig, frame: &Frame) -> Result<(i32, i32), io::Error> {
    let (mut horizontal, mut vertical) = if let Some(percent) = config.crop_percent {
        (
            u64::from(frame.width) * u64::from(percent) / 200,
            u64::from(frame.height) * u64::from(percent) / 200,
        )
    } else {
        let horizontal = u64::try_from(config.border_crop).unwrap_or_default();
        (
            horizontal,
            horizontal * u64::from(frame.height) / u64::from(frame.width.max(1)),
        )
    };
    if matches!(frame.format, VideoFormat::I420 | VideoFormat::NV12) {
        horizontal = horizontal / 2 * 2;
        vertical = vertical / 2 * 2;
    }
    if horizontal.saturating_mul(2) >= u64::from(frame.width) {
        return Err(invalid_frame("Horizontal crop exceeds frame width"));
    }
    if vertical.saturating_mul(2) >= u64::from(frame.height) {
        return Err(invalid_frame("Vertical crop exceeds frame height"));
    }
    Ok((
        i32::try_from(horizontal).map_err(|_| invalid_frame("Horizontal crop is too large"))?,
        i32::try_from(vertical).map_err(|_| invalid_frame("Vertical crop is too large"))?,
    ))
}

fn frame_to_bgra(frame: &Frame) -> Result<Mat, Box<dyn std::error::Error + Send + Sync>> {
    let width =
        i32::try_from(frame.width).map_err(|_| invalid_frame("frame width is too large"))?;
    let height =
        i32::try_from(frame.height).map_err(|_| invalid_frame("frame height is too large"))?;
    if width <= 0 || height <= 0 {
        return Err(invalid_frame("frame dimensions must be non-zero").into());
    }

    let (source, conversion) = match frame.format {
        VideoFormat::I420 | VideoFormat::NV12 => {
            if width % 2 != 0 || height % 2 != 0 {
                return Err(invalid_frame("I420 and NV12 dimensions must be even").into());
            }
            let required_len = usize::try_from(width)
                .ok()
                .and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
                .and_then(|pixels| pixels.checked_mul(3))
                .map(|bytes| bytes / 2)
                .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
            if frame.data.len() < required_len {
                return Err(invalid_frame("frame buffer is too small for its dimensions").into());
            }
            let yuv = Mat::from_slice(&frame.data[..required_len])?
                .reshape(1, height * 3 / 2)?
                .try_clone()?;
            let conversion = if matches!(frame.format, VideoFormat::I420) {
                imgproc::COLOR_YUV2BGRA_I420
            } else {
                imgproc::COLOR_YUV2BGRA_NV12
            };
            (yuv, conversion)
        }
        VideoFormat::RGB | VideoFormat::RGBA | VideoFormat::BGR | VideoFormat::BGRA => {
            let channels = match frame.format {
                VideoFormat::RGB | VideoFormat::BGR => 3,
                VideoFormat::RGBA | VideoFormat::BGRA => 4,
                VideoFormat::I420 | VideoFormat::NV12 => unreachable!(),
            };
            let row_bytes = usize::try_from(width)
                .ok()
                .and_then(|value| value.checked_mul(channels))
                .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
            let rows =
                usize::try_from(height).map_err(|_| invalid_frame("frame height is too large"))?;
            let stride = packed_row_stride(frame.data.len(), row_bytes, rows)?;
            let mut source = Mat::new_rows_cols_with_default(
                height,
                width,
                match channels {
                    3 => core::CV_8UC3,
                    4 => core::CV_8UC4,
                    _ => unreachable!(),
                },
                Scalar::all(0.0),
            )?;
            let source_bytes = source.data_bytes_mut()?;
            for row in 0..rows {
                let source_start = row * stride;
                let target_start = row * row_bytes;
                source_bytes[target_start..target_start + row_bytes]
                    .copy_from_slice(&frame.data[source_start..source_start + row_bytes]);
            }
            let conversion = match frame.format {
                VideoFormat::RGB => imgproc::COLOR_RGB2BGRA,
                VideoFormat::BGR => imgproc::COLOR_BGR2BGRA,
                VideoFormat::RGBA => imgproc::COLOR_RGBA2BGRA,
                VideoFormat::BGRA => {
                    return Ok(source);
                }
                VideoFormat::I420 | VideoFormat::NV12 => unreachable!(),
            };
            (source, conversion)
        }
    };
    let mut bgra = Mat::default();
    imgproc::cvt_color(&source, &mut bgra, conversion, 0)?;
    Ok(bgra)
}

fn bgra_to_format(
    bgra: &Mat,
    format: VideoFormat,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    if matches!(format, VideoFormat::NV12) {
        let mut i420 = Mat::default();
        imgproc::cvt_color(bgra, &mut i420, imgproc::COLOR_BGRA2YUV_I420, 0)?;
        let width =
            usize::try_from(bgra.cols()).map_err(|_| invalid_frame("frame width is too large"))?;
        let height =
            usize::try_from(bgra.rows()).map_err(|_| invalid_frame("frame height is too large"))?;
        let luma_len = width
            .checked_mul(height)
            .ok_or_else(|| invalid_frame("frame dimensions overflow"))?;
        let chroma_len = luma_len / 4;
        let planar = i420.data_bytes()?;
        let mut output = Vec::with_capacity(luma_len + 2 * chroma_len);
        output.extend_from_slice(&planar[..luma_len]);
        for index in 0..chroma_len {
            output.push(planar[luma_len + index]);
            output.push(planar[luma_len + chroma_len + index]);
        }
        return Ok(output);
    }
    let mut output = Mat::default();
    let conversion = match format {
        VideoFormat::BGRA => {
            bgra.copy_to(&mut output)?;
            return Ok(output.data_bytes()?.to_vec());
        }
        VideoFormat::RGB => imgproc::COLOR_BGRA2RGB,
        VideoFormat::BGR => imgproc::COLOR_BGRA2BGR,
        VideoFormat::RGBA => imgproc::COLOR_BGRA2RGBA,
        VideoFormat::I420 => imgproc::COLOR_BGRA2YUV_I420,
        VideoFormat::NV12 => unreachable!(),
    };
    imgproc::cvt_color(bgra, &mut output, conversion, 0)?;
    Ok(output.data_bytes()?.to_vec())
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

    use super::{MIN_MATCHES, StabilizationConfig, StabilizationMiddleware};
    use crate::{
        frame::{Frame, MotionMatch, VideoFormat},
        middleware::Middleware,
    };

    fn frame_with_motion(motion: Vec<MotionMatch>) -> Frame {
        let mut pixels = Vec::with_capacity(64 * 48 * 4);
        for y in 0..48 {
            for x in 0..64 {
                let value = (x + y) as u8;
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let frame = Frame::new(Bytes::from(pixels), 64, 48, VideoFormat::BGRA);
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .motion_matches = Some(motion);
        frame
    }

    fn translated_matches(dx: f32, dy: f32) -> Vec<MotionMatch> {
        (0..20)
            .map(|index| {
                let x = 5.0 + (index % 5) as f32 * 10.0;
                let y = 5.0 + (index / 5) as f32 * 10.0;
                ((x, y), (x + dx, y + dy))
            })
            .collect()
    }

    #[tokio::test]
    async fn passes_through_without_ten_matches() {
        let middleware = StabilizationMiddleware::default();
        let frame = frame_with_motion(translated_matches(2.0, 1.0)[..MIN_MATCHES - 1].to_vec());
        let original = frame.data.clone();
        let processed = middleware
            .process_async(frame)
            .await
            .expect("frame should pass through");
        assert!(Arc::ptr_eq(&processed.data, &original));
    }

    #[test]
    fn estimates_rotation_scale_and_unscaled_translation() {
        let angle = 0.1_f32;
        let scale = 1.05_f32;
        let dx = 2.0_f32;
        let dy = -1.0_f32;
        let matches = (0..20)
            .map(|index| {
                let x = 10.0 + (index % 5) as f32 * 20.0;
                let y = 10.0 + (index / 5) as f32 * 20.0;
                let current = (
                    scale * (angle.cos() * x - angle.sin() * y) + dx,
                    scale * (angle.sin() * x + angle.cos() * y) + dy,
                );
                ((x, y), current)
            })
            .collect::<Vec<MotionMatch>>();

        let measured = super::estimate_motion(&matches, 0.25)
            .expect("affine estimation should succeed")
            .expect("sufficient matches should produce a transform");
        assert!((measured.dx - 8.0).abs() < 0.1);
        assert!((measured.dy + 4.0).abs() < 0.1);
        assert!((measured.da - f64::from(angle)).abs() < 0.001);
        assert!((measured.scale - 0.05).abs() < 0.001);
    }

    #[tokio::test]
    async fn warps_and_outputs_cropped_dimensions_and_format() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            border_crop: 5,
            frame_scale: 1.0,
            ..StabilizationConfig::default()
        })
        .expect("valid stabilization config");
        let frame = frame_with_motion(translated_matches(2.0, 1.0));
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .feature_points = Some(vec![(30, 20)]);
        let processed = middleware
            .process_async(frame)
            .await
            .expect("frame should be stabilized");
        assert_eq!((processed.width, processed.height), (54, 42));
        assert!(matches!(processed.format, VideoFormat::BGRA));
        assert_eq!(processed.data.len(), 54 * 42 * 4);
        assert!(
            processed
                .metadata
                .read()
                .expect("metadata lock should not be poisoned")
                .estimated_transform
                .is_some()
        );
        assert_eq!(
            processed
                .metadata
                .read()
                .expect("metadata lock should not be poisoned")
                .feature_points
                .as_deref(),
            Some(&[(23, 16)][..])
        );
    }

    #[tokio::test]
    async fn falls_back_to_opencv_when_gpu_is_unavailable() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            border_crop: 0,
            frame_scale: 1.0,
            ..StabilizationConfig::default()
        })
        .expect("valid stabilization config");
        middleware
            .gpu
            .set(None)
            .expect("GPU context should not be initialized yet");

        let processed = middleware
            .process_async(frame_with_motion(translated_matches(2.0, 1.0)))
            .await
            .expect("CPU fallback should stabilize the frame");

        assert_eq!((processed.width, processed.height), (64, 48));
        assert_eq!(processed.data.len(), 64 * 48 * 4);
    }

    #[tokio::test]
    async fn default_crop_produces_aspect_ratio_adjusted_output_dimensions() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            frame_scale: 1.0,
            ..StabilizationConfig::default()
        })
        .expect("valid stabilization config");
        let mut pixels = vec![128; 640 * 480 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        let frame = Frame::new(Bytes::from(pixels), 640, 480, VideoFormat::BGRA);
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .motion_matches = Some(translated_matches(2.0, 1.0));

        let processed = middleware
            .process_async(frame)
            .await
            .expect("frame should be stabilized");
        assert_eq!((processed.width, processed.height), (540, 406));
        assert_eq!(processed.data.len(), 540 * 406 * 4);
    }

    #[tokio::test]
    async fn crop_percent_controls_output_resolution() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            crop_percent: Some(80),
            frame_scale: 1.0,
            ..StabilizationConfig::default()
        })
        .expect("valid stabilization config");
        let frame = Frame::new(
            Bytes::from(vec![128; 640 * 480 * 4]),
            640,
            480,
            VideoFormat::BGRA,
        );
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .motion_matches = Some(translated_matches(2.0, 1.0));

        let processed = middleware
            .process_async(frame)
            .await
            .expect("frame should be stabilized");
        assert_eq!((processed.width, processed.height), (128, 96));
    }

    #[tokio::test]
    async fn rejects_crop_that_exceeds_frame_dimensions() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            border_crop: 33,
            frame_scale: 1.0,
            ..StabilizationConfig::default()
        })
        .expect("config validates independently of input dimensions");
        let frame = frame_with_motion(translated_matches(2.0, 1.0));
        assert!(middleware.process_async(frame).await.is_err());
    }

    #[tokio::test]
    async fn preserves_packed_and_yuv_pixel_formats() {
        let middleware = StabilizationMiddleware::new(StabilizationConfig {
            border_crop: 0,
            frame_scale: 1.0,
            ..StabilizationConfig::default()
        })
        .expect("valid stabilization config");
        let matches = (0..10)
            .map(|index| {
                let point = (1.0 + (index % 5) as f32, 1.0 + (index / 5) as f32 * 5.0);
                (point, point)
            })
            .collect::<Vec<MotionMatch>>();

        for (format, bytes_per_pixel) in [
            (VideoFormat::RGB, 3),
            (VideoFormat::BGR, 3),
            (VideoFormat::RGBA, 4),
            (VideoFormat::BGRA, 4),
            (VideoFormat::I420, 0),
            (VideoFormat::NV12, 0),
        ] {
            let len = if bytes_per_pixel == 0 {
                8 * 8 * 3 / 2
            } else {
                8 * 8 * bytes_per_pixel
            };
            let frame = Frame::new(Bytes::from(vec![128; len]), 8, 8, format.clone());
            frame
                .metadata
                .write()
                .expect("metadata lock should not be poisoned")
                .motion_matches = Some(matches.clone());

            let processed = middleware
                .process_async(frame)
                .await
                .expect("supported pixel format should be stabilized");
            assert_eq!(
                std::mem::discriminant(&processed.format),
                std::mem::discriminant(&format)
            );
            assert_eq!((processed.width, processed.height), (8, 8));
            assert_eq!(processed.data.len(), len);
        }
    }

    #[test]
    fn rejects_invalid_noise_scale_and_frame_scale() {
        assert!(
            StabilizationMiddleware::new(StabilizationConfig {
                q_scale: f64::NAN,
                ..StabilizationConfig::default()
            })
            .is_err()
        );
        assert!(
            StabilizationMiddleware::new(StabilizationConfig {
                frame_scale: 0.0,
                ..StabilizationConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn test_kalman_convergence() {
        use crate::middleware::stabilization::{KalmanState, Measurement};

        let mut state = KalmanState::new(&StabilizationConfig {
            q_scale: 0.001, // Aggressive
            r_scale: 1.5,
            ..Default::default()
        });

        // Feed constant motion
        let motion = Measurement {
            dx: 10.0,
            dy: 5.0,
            da: 0.0,
            scale: 0.0,
        };

        let result1 = state.update(motion);
        let result2 = state.update(motion);
        let result3 = state.update(motion);

        // Should converge toward measurement
        assert!((result1.dx - 10.0).abs() > (result3.dx - 10.0).abs());
        println!(
            "Smoothed DX: {} → {} → {}",
            result1.dx, result2.dx, result3.dx
        );
    }
}
