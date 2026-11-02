use std::sync::{Arc, Mutex};

use bytes::Bytes;
use purecv::{
    core::{
        Matrix,
        types::{BorderTypes, Point2f, Scalar, Size2i, TermCriteria, TermType},
    },
    imgproc::{
        InterpolationFlags, cvt_color_bgr_to_gray, cvt_color_bgra_to_gray,
        geometric::warp_perspective,
    },
    video::optical_flow::calc_optical_flow_pyramid_lk,
};

use crate::{
    frame::{Frame, MotionMatch, VideoFormat},
    middleware::{Middleware, ProcessResult},
};

const MIN_MATCHES: usize = 10;
const RANSAC_ITERATIONS: usize = 2_000;
const RANSAC_THRESHOLD: f64 = 3.0;

#[derive(Debug)]
pub struct WarpMiddleware {
    frame_scale: f32,
    crop_percent: u8,
    state: Arc<Mutex<WarpState>>,
}

#[derive(Debug, Default)]
struct WarpState {
    previous_gray: Option<Matrix<u8>>,
    previous_points: Vec<Point2f>,
    smoothed_motion: Option<SimilarityTransform>,
}

impl WarpMiddleware {
    pub fn new(frame_scale: f32) -> anyhow::Result<Self> {
        if !frame_scale.is_finite() || frame_scale <= 0.0 || frame_scale > 1.0 {
            return Err(anyhow::anyhow!(
                "Frame scale must be finite and in (0.0, 1.0]"
            ));
        }

        Ok(Self {
            frame_scale,
            crop_percent: 0,
            state: Arc::new(Mutex::new(WarpState::default())),
        })
    }

    pub fn with_crop_percent(mut self, crop_percent: u8) -> anyhow::Result<Self> {
        if crop_percent >= 100 {
            return Err(anyhow::anyhow!("Crop percentage must be in [0, 100)"));
        }
        self.crop_percent = crop_percent;
        Ok(self)
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let width = i32::try_from(frame.width)
            .map_err(|_| anyhow::anyhow!("Frame width exceeds purecv limits"))?;
        let height = i32::try_from(frame.height)
            .map_err(|_| anyhow::anyhow!("Frame height exceeds purecv limits"))?;
        if width <= 0 || height <= 0 {
            return Err(anyhow::anyhow!("Frame dimensions must be non-zero"));
        }

        let current_gray = frame_grayscale(&frame, self.frame_scale)?;
        let current_points = frame
            .metadata
            .feature_points
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|&(x, y)| Point2f::new(x as f32, y as f32))
            .collect::<Vec<_>>();
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Warp state lock is poisoned"))?;
        let mut matches = track_points(&state, &current_gray)?;
        matches.extend(
            frame
                .metadata
                .motion_matches
                .as_deref()
                .unwrap_or_default()
                .iter()
                .copied(),
        );
        let motion = estimate_motion(&matches, self.frame_scale).or_else(|| {
            frame
                .metadata
                .motion_vectors
                .as_deref()
                .filter(|vectors| vectors.len() >= MIN_MATCHES)
                .and_then(|vectors| estimate_translation(vectors, self.frame_scale))
        });
        let motion = motion.map(|motion| smooth_motion(&mut state, motion));
        state.previous_gray = Some(current_gray);
        state.previous_points = current_points;
        let inverse = motion.and_then(inverse_transform);
        let identity = Matrix::from_vec(3, 3, 1, vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);

        let channels = frame.format.channels();
        let row_bytes = frame
            .width
            .checked_mul(channels)
            .ok_or_else(|| anyhow::anyhow!("Frame width dimension overflow"))?;
        let packed_len = row_bytes
            .checked_mul(frame.height)
            .ok_or_else(|| anyhow::anyhow!("Frame height dimension overflow"))?;
        if frame.data.len() < packed_len {
            return Err(anyhow::anyhow!(
                "Frame buffer is too small for its dimensions and pixel format"
            ));
        }

        let row_stride = if frame.height > 0
            && frame.data.len().is_multiple_of(frame.height)
            && frame.data.len() / frame.height >= row_bytes
        {
            frame.data.len() / frame.height
        } else {
            row_bytes
        };
        let mut packed = Vec::with_capacity(packed_len);
        for row in 0..frame.height {
            let start = row * row_stride;
            packed.extend_from_slice(&frame.data[start..start + row_bytes]);
        }

        let image = Matrix::from_vec(frame.height, frame.width, channels, packed);
        let transform = inverse.as_ref().unwrap_or(&identity);
        let warped = warp_perspective(
            &image,
            transform,
            Size2i::new(width, height),
            InterpolationFlags::Linear,
            BorderTypes::Reflect,
            Scalar::all(0),
        )
        .map_err(|error| anyhow::anyhow!("purecv image warp failed: {error}"))?;

        let (cropped, crop_x, crop_y) = crop_image(&warped, self.crop_percent)?;
        frame.width = cropped.cols;
        frame.height = cropped.rows;
        frame.data = Arc::new(Bytes::from(cropped.data));
        if inverse.is_some() {
            let estimated_transform = [
                (transform.data[0]) as f32,
                (transform.data[1]) as f32,
                (transform.data[2] - crop_x as f64) as f32,
                (transform.data[3]) as f32,
                (transform.data[4]) as f32,
                (transform.data[5] - crop_y as f64) as f32,
            ];
            if estimated_transform.iter().any(|value| !value.is_finite()) {
                return Err(anyhow::anyhow!(
                    "Estimated image transform exceeds supported limits"
                ));
            }
            frame.metadata.estimated_transform = Some(estimated_transform);
        }
        if let Some(points) = frame.metadata.feature_points.as_mut() {
            let feature_scale = f64::from(self.frame_scale);
            points.retain_mut(|(x, y)| {
                let transformed_x = transform.data[0] * f64::from(*x)
                    + transform.data[1] * f64::from(*y)
                    + (transform.data[2] - crop_x as f64) * feature_scale;
                let transformed_y = transform.data[3] * f64::from(*x)
                    + transform.data[4] * f64::from(*y)
                    + (transform.data[5] - crop_y as f64) * feature_scale;
                let rounded_x = transformed_x.round();
                let rounded_y = transformed_y.round();
                if !rounded_x.is_finite()
                    || !rounded_y.is_finite()
                    || rounded_x < 0.0
                    || rounded_y < 0.0
                    || rounded_x >= cropped.cols as f64 * feature_scale
                    || rounded_y >= cropped.rows as f64 * feature_scale
                {
                    return false;
                }

                *x = rounded_x as u32;
                *y = rounded_y as u32;
                true
            });
        }

        Ok(frame)
    }
}

impl Default for WarpMiddleware {
    fn default() -> Self {
        Self::new(0.25).expect("valid frame scale")
    }
}

impl Middleware for WarpMiddleware {
    fn name(&self) -> &'static str {
        "warp"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        let frame_scale = self.frame_scale;
        let crop_percent = self.crop_percent;
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            WarpMiddleware {
                frame_scale,
                crop_percent,
                state,
            }
            .process_frame(frame)
        })
        .await?
        .map_err(Into::into)
    }
}

fn frame_grayscale(frame: &Frame, frame_scale: f32) -> anyhow::Result<Matrix<u8>> {
    let width = ((frame.width as f32 * frame_scale).round() as usize).max(1);
    let height = ((frame.height as f32 * frame_scale).round() as usize).max(1);
    let gray = if let Some(gray) = frame.metadata.grayscale.as_deref() {
        gray.clone()
    } else {
        let channels = frame.format.channels();
        let row_bytes = frame
            .width
            .checked_mul(channels)
            .ok_or_else(|| anyhow::anyhow!("Frame row size overflow"))?;
        let packed_len = row_bytes
            .checked_mul(frame.height)
            .ok_or_else(|| anyhow::anyhow!("Frame buffer size overflow"))?;
        if frame.data.len() < packed_len {
            return Err(anyhow::anyhow!(
                "Frame buffer is too small for its dimensions and pixel format"
            ));
        }
        let stride = if frame.data.len().is_multiple_of(frame.height)
            && frame.data.len() / frame.height >= row_bytes
        {
            frame.data.len() / frame.height
        } else {
            row_bytes
        };
        let mut packed = Vec::with_capacity(packed_len);
        for row in 0..frame.height {
            let start = row * stride;
            packed.extend_from_slice(&frame.data[start..start + row_bytes]);
        }
        let color = Matrix::from_vec(frame.height, frame.width, channels, packed);
        match frame.format {
            VideoFormat::BGR => cvt_color_bgr_to_gray(&color).map_err(anyhow::Error::msg)?,
            VideoFormat::BGRA => cvt_color_bgra_to_gray(&color).map_err(anyhow::Error::msg)?,
        }
    };
    if gray.channels != 1 {
        return Err(anyhow::anyhow!(
            "Warp input grayscale image must have one channel"
        ));
    }

    if gray.cols == width && gray.rows == height {
        Ok(gray)
    } else {
        Ok(purecv::imgproc::resize(
            &gray,
            purecv::core::Size::new(width, height),
        )?)
    }
}

fn track_points(state: &WarpState, current_gray: &Matrix<u8>) -> anyhow::Result<Vec<MotionMatch>> {
    let Some(previous_gray) = state.previous_gray.as_ref() else {
        return Ok(Vec::new());
    };
    if previous_gray.cols != current_gray.cols
        || previous_gray.rows != current_gray.rows
        || state.previous_points.len() < MIN_MATCHES
    {
        return Ok(Vec::new());
    }

    let window = Size2i::new(21, 21);
    if current_gray.cols < window.width as usize || current_gray.rows < window.height as usize {
        return Ok(Vec::new());
    }
    let (tracked, status, _) = calc_optical_flow_pyramid_lk(
        previous_gray,
        current_gray,
        &state.previous_points,
        None,
        window,
        3,
        TermCriteria::new(TermType::Both, 30, 0.01),
        0,
        1e-4,
    )?;

    Ok(state
        .previous_points
        .iter()
        .zip(tracked.iter())
        .zip(status.iter())
        .filter_map(|((&previous, &current), &status)| {
            (status != 0
                && current.x.is_finite()
                && current.y.is_finite()
                && current.x >= 0.0
                && current.y >= 0.0
                && current.x < current_gray.cols as f32
                && current.y < current_gray.rows as f32)
                .then_some(((previous.x, previous.y), (current.x, current.y)))
        })
        .collect())
}

fn smooth_motion(state: &mut WarpState, motion: SimilarityTransform) -> SimilarityTransform {
    let Some(previous) = state.smoothed_motion else {
        state.smoothed_motion = Some(motion);
        return motion;
    };
    const CURRENT_WEIGHT: f64 = 0.75;
    let previous_weight = 1.0 - CURRENT_WEIGHT;
    let smoothed = SimilarityTransform {
        a: motion.a * CURRENT_WEIGHT + previous.a * previous_weight,
        b: motion.b * CURRENT_WEIGHT + previous.b * previous_weight,
        tx: motion.tx * CURRENT_WEIGHT + previous.tx * previous_weight,
        ty: motion.ty * CURRENT_WEIGHT + previous.ty * previous_weight,
    };
    state.smoothed_motion = Some(smoothed);
    smoothed
}

fn crop_image(image: &Matrix<u8>, crop_percent: u8) -> anyhow::Result<(Matrix<u8>, usize, usize)> {
    let crop_x = image.cols * usize::from(crop_percent) / 200;
    let crop_y = image.rows * usize::from(crop_percent) / 200;
    let width = image.cols.saturating_sub(crop_x.saturating_mul(2));
    let height = image.rows.saturating_sub(crop_y.saturating_mul(2));
    if width == 0 || height == 0 {
        return Err(anyhow::anyhow!(
            "Crop percentage leaves no output pixels for this frame"
        ));
    }

    let row_len = width
        .checked_mul(image.channels)
        .ok_or_else(|| anyhow::anyhow!("Cropped image row size overflow"))?;
    let mut pixels = Vec::with_capacity(
        row_len
            .checked_mul(height)
            .ok_or_else(|| anyhow::anyhow!("Cropped image size overflow"))?,
    );
    for row in crop_y..crop_y + height {
        let start = (row * image.cols + crop_x) * image.channels;
        pixels.extend_from_slice(&image.data[start..start + row_len]);
    }
    Ok((
        Matrix::from_vec(height, width, image.channels, pixels),
        crop_x,
        crop_y,
    ))
}

#[derive(Debug, Clone, Copy)]
struct SimilarityTransform {
    a: f64,
    b: f64,
    tx: f64,
    ty: f64,
}

fn estimate_motion(matches: &[MotionMatch], frame_scale: f32) -> Option<SimilarityTransform> {
    let matches: Vec<_> = matches
        .iter()
        .copied()
        .filter(|((px, py), (cx, cy))| [px, py, cx, cy].into_iter().all(|value| value.is_finite()))
        .collect();
    if matches.len() < MIN_MATCHES {
        return None;
    }

    let mut random_state = (matches.len() ^ 0x9e37_79b9).max(1);
    let mut best_model = None;
    let mut best_inlier_count = 0;
    for _ in 0..RANSAC_ITERATIONS {
        let first = next_random(&mut random_state) % matches.len();
        let mut second = next_random(&mut random_state) % matches.len();
        if first == second {
            second = (second + 1) % matches.len();
        }
        let Some(candidate) = fit_similarity(&[matches[first], matches[second]]) else {
            continue;
        };
        let inlier_count = matches
            .iter()
            .filter(|matched| {
                reprojection_error_squared(candidate, **matched) <= RANSAC_THRESHOLD.powi(2)
            })
            .count();
        if inlier_count > best_inlier_count {
            best_model = Some(candidate);
            best_inlier_count = inlier_count;
        }
    }
    if best_inlier_count < MIN_MATCHES {
        return None;
    }

    let mut model = best_model?;
    for _ in 0..2 {
        let inliers: Vec<_> = matches
            .iter()
            .copied()
            .filter(|matched| {
                reprojection_error_squared(model, *matched) <= RANSAC_THRESHOLD.powi(2)
            })
            .collect();
        if inliers.len() < MIN_MATCHES {
            return None;
        }
        model = fit_similarity(&inliers)?;
    }

    let translation_scale = f64::from(frame_scale);
    model.tx /= translation_scale;
    model.ty /= translation_scale;
    [model.a, model.b, model.tx, model.ty]
        .into_iter()
        .all(f64::is_finite)
        .then_some(model)
}

fn next_random(state: &mut usize) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn fit_similarity(matches: &[MotionMatch]) -> Option<SimilarityTransform> {
    if matches.len() < 2 {
        return None;
    }

    let count = matches.len() as f64;
    let (previous_x, previous_y, current_x, current_y) = matches.iter().fold(
        (0.0, 0.0, 0.0, 0.0),
        |(sum_px, sum_py, sum_cx, sum_cy), &((px, py), (cx, cy))| {
            (
                sum_px + f64::from(px),
                sum_py + f64::from(py),
                sum_cx + f64::from(cx),
                sum_cy + f64::from(cy),
            )
        },
    );
    let (previous_x, previous_y, current_x, current_y) = (
        previous_x / count,
        previous_y / count,
        current_x / count,
        current_y / count,
    );

    let (a_numerator, b_numerator, denominator) = matches.iter().fold(
        (0.0, 0.0, 0.0),
        |(sum_a, sum_b, sum_d), &((px, py), (cx, cy))| {
            let x = f64::from(px) - previous_x;
            let y = f64::from(py) - previous_y;
            let u = f64::from(cx) - current_x;
            let v = f64::from(cy) - current_y;
            (
                sum_a + x * u + y * v,
                sum_b + x * v - y * u,
                sum_d + x * x + y * y,
            )
        },
    );
    if denominator <= f64::EPSILON {
        return None;
    }

    let a = a_numerator / denominator;
    let b = b_numerator / denominator;
    let model = SimilarityTransform {
        a,
        b,
        tx: current_x - a * previous_x + b * previous_y,
        ty: current_y - b * previous_x - a * previous_y,
    };
    [model.a, model.b, model.tx, model.ty]
        .into_iter()
        .all(f64::is_finite)
        .then_some(model)
}

fn reprojection_error_squared(model: SimilarityTransform, matched: MotionMatch) -> f64 {
    let ((x, y), (observed_x, observed_y)) = matched;
    let x = f64::from(x);
    let y = f64::from(y);
    let error_x = model.a * x - model.b * y + model.tx - f64::from(observed_x);
    let error_y = model.b * x + model.a * y + model.ty - f64::from(observed_y);
    error_x * error_x + error_y * error_y
}

fn estimate_translation(vectors: &[(i16, i16)], frame_scale: f32) -> Option<SimilarityTransform> {
    let count = vectors.len() as f64;
    let scale = f64::from(frame_scale);
    Some(SimilarityTransform {
        a: 1.0,
        b: 0.0,
        tx: vectors.iter().map(|(dx, _)| f64::from(*dx)).sum::<f64>() / count / scale,
        ty: vectors.iter().map(|(_, dy)| f64::from(*dy)).sum::<f64>() / count / scale,
    })
}

fn inverse_transform(motion: SimilarityTransform) -> Option<Matrix<f64>> {
    let determinant = motion.a * motion.a + motion.b * motion.b;
    if !determinant.is_finite() || determinant <= f64::EPSILON {
        return None;
    }
    let a = motion.a / determinant;
    let b = motion.b / determinant;
    let tx = -a * motion.tx - b * motion.ty;
    let ty = b * motion.tx - a * motion.ty;
    let coefficients = [a, b, tx, -b, a, ty];
    coefficients
        .into_iter()
        .all(f64::is_finite)
        .then(|| Matrix::from_vec(3, 3, 1, vec![a, b, tx, -b, a, ty, 0.0, 0.0, 1.0]))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{MIN_MATCHES, WarpMiddleware};
    use crate::{
        frame::{Frame, MotionMatch, VideoFormat},
        middleware::Middleware,
    };

    fn translated_matches(dx: f32, dy: f32) -> Vec<MotionMatch> {
        (0_u8..20)
            .map(|index| {
                let x = 5.0 + f32::from(index % 5) * 10.0;
                let y = 5.0 + f32::from(index / 5) * 10.0;
                ((x, y), (x + dx, y + dy))
            })
            .collect()
    }

    #[tokio::test]
    async fn applies_identity_when_there_are_no_motion_matches() {
        let mut frame = Frame::new(Bytes::from(vec![80; 16 * 16 * 3]), 16, 16, VideoFormat::BGR);
        frame.metadata.motion_matches =
            Some(translated_matches(2.0, 1.0)[..MIN_MATCHES - 1].to_vec());

        let processed = WarpMiddleware::new(1.0)
            .expect("valid frame scale")
            .process_async(frame)
            .await
            .expect("frame should pass through");

        assert_eq!((processed.width, processed.height), (16, 16));
        assert_eq!(processed.data.len(), 16 * 16 * 3);
        assert!(processed.metadata.estimated_transform.is_none());
    }

    #[tokio::test]
    async fn crops_output_resolution_by_configured_percentage() {
        let frame = Frame::new(Bytes::from(vec![80; 80 * 40 * 3]), 80, 40, VideoFormat::BGR);
        let processed = WarpMiddleware::new(0.25)
            .expect("valid frame scale")
            .with_crop_percent(25)
            .expect("valid crop percentage")
            .process_async(frame)
            .await
            .expect("crop should succeed");

        assert_eq!((processed.width, processed.height), (60, 30));
        assert_eq!(processed.data.len(), 60 * 30 * 3);
    }

    #[tokio::test]
    async fn tracks_features_between_frames_and_stabilizes_translation() {
        let width = 96;
        let height = 96;
        let mut state = 0x1234_5678_u32;
        let mut source = vec![0_u8; width * height];
        for pixel in &mut source {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *pixel = (state >> 24) as u8;
        }
        let points = (20..80)
            .step_by(10)
            .flat_map(|y| (20..80).step_by(10).map(move |x| (x as u32, y as u32)))
            .collect::<Vec<_>>();
        let middleware = WarpMiddleware::new(1.0).expect("valid frame scale");

        let mut previous = Frame::new(
            Bytes::from(
                source
                    .iter()
                    .flat_map(|&gray| [gray, gray, gray])
                    .collect::<Vec<_>>(),
            ),
            width,
            height,
            VideoFormat::BGR,
        );
        previous.metadata.feature_points = Some(points.clone());
        middleware
            .process_async(previous)
            .await
            .expect("first frame should be accepted");

        let mut translated = vec![0_u8; width * height];
        for y in 0..height {
            for x in 0..width {
                let source_x = x.saturating_sub(2);
                let source_y = (y + 1).min(height - 1);
                translated[y * width + x] = source[source_y * width + source_x];
            }
        }
        let mut current = Frame::new(
            Bytes::from(
                translated
                    .iter()
                    .flat_map(|&gray| [gray, gray, gray])
                    .collect::<Vec<_>>(),
            ),
            width,
            height,
            VideoFormat::BGR,
        );
        current.metadata.feature_points = Some(points);

        let stabilized = middleware
            .process_async(current)
            .await
            .expect("tracked frame should be stabilized");
        let transform = stabilized
            .metadata
            .estimated_transform
            .expect("optical flow should estimate a transform");
        assert!((transform[2] + 2.0).abs() < 0.75);
        assert!((transform[5] - 1.0).abs() < 0.75);
    }

    #[tokio::test]
    async fn warps_pixels_and_transforms_feature_points() {
        let pixels = (0..32 * 24)
            .flat_map(|index| {
                let value = u8::try_from(index % 32).expect("pixel column fits in u8") * 4;
                [value, value, value]
            })
            .collect::<Vec<_>>();
        let mut frame = Frame::new(Bytes::from(pixels), 32, 24, VideoFormat::BGR);
        frame.metadata.motion_matches = Some(translated_matches(2.0, 1.0));
        frame.metadata.feature_points = Some(vec![(15, 12)]);

        let processed = WarpMiddleware::new(1.0)
            .expect("valid frame scale")
            .process_async(frame)
            .await
            .expect("frame should be warped");

        assert_eq!((processed.width, processed.height), (32, 24));
        assert_eq!(processed.data.len(), 32 * 24 * 3);
        assert_eq!(
            processed.metadata.feature_points.as_deref(),
            Some(&[(13, 11)][..])
        );
        assert!((processed.metadata.estimated_transform.expect("transform")[2] + 2.0).abs() < 0.01);
        assert_eq!(processed.data[(12 * 32 + 10) * 3], 48);
    }

    #[tokio::test]
    async fn estimates_motion_robustly_with_outlier_matches() {
        let mut matches = translated_matches(2.0, -1.0);
        matches[0] = ((5.0, 5.0), (100.0, 80.0));
        let mut frame = Frame::new(
            Bytes::from(vec![100; 32 * 24 * 4]),
            32,
            24,
            VideoFormat::BGRA,
        );
        frame.metadata.motion_matches = Some(matches);

        let processed = WarpMiddleware::new(1.0)
            .expect("valid frame scale")
            .process_async(frame)
            .await
            .expect("outlier should not prevent a warp");

        let transform = processed.metadata.estimated_transform.expect("transform");
        assert!((transform[2] + 2.0).abs() < 0.01);
        assert!((transform[5] - 1.0).abs() < 0.01);
        assert_eq!(processed.data.len(), 32 * 24 * 4);
    }

    #[tokio::test]
    async fn falls_back_to_average_motion_vectors() {
        let mut frame = Frame::new(
            Bytes::from(vec![100; 32 * 24 * 3]),
            32,
            24,
            VideoFormat::BGR,
        );
        frame.metadata.motion_vectors = Some(vec![(1, -2); MIN_MATCHES]);

        let processed = WarpMiddleware::new(0.5)
            .expect("valid frame scale")
            .process_async(frame)
            .await
            .expect("motion vectors should provide a translation");

        let transform = processed.metadata.estimated_transform.expect("transform");
        assert!((transform[2] + 2.0).abs() < 0.01);
        assert!((transform[5] - 4.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn corrects_rotation_and_scale() {
        let angle = 0.1_f32;
        let scale = 1.05_f32;
        let matches = (0_u8..20)
            .map(|index| {
                let x = 10.0 + f32::from(index % 5) * 20.0;
                let y = 10.0 + f32::from(index / 5) * 20.0;
                let current = (
                    scale * (angle.cos() * x - angle.sin() * y) + 2.0,
                    scale * (angle.sin() * x + angle.cos() * y) - 1.0,
                );
                ((x, y), current)
            })
            .collect();
        let mut frame = Frame::new(
            Bytes::from(vec![100; 32 * 24 * 3]),
            32,
            24,
            VideoFormat::BGR,
        );
        frame.metadata.motion_matches = Some(matches);

        let processed = WarpMiddleware::new(1.0)
            .expect("valid frame scale")
            .process_async(frame)
            .await
            .expect("frame should be warped");

        let transform = processed.metadata.estimated_transform.expect("transform");
        assert!(
            (f64::from(transform[0]) - f64::from(angle.cos()) / f64::from(scale)).abs() < 0.001
        );
        assert!(
            (f64::from(transform[1]) - f64::from(angle.sin()) / f64::from(scale)).abs() < 0.001
        );
    }
}
