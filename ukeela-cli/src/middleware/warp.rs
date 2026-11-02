use std::sync::Arc;

use bytes::Bytes;
use purecv::{
    core::{
        Matrix,
        types::{BorderTypes, Scalar, Size2i},
    },
    imgproc::{InterpolationFlags, geometric::warp_perspective},
};

use crate::{
    frame::{Frame, MotionMatch},
    middleware::{Middleware, ProcessResult},
};

const MIN_MATCHES: usize = 10;
const RANSAC_ITERATIONS: usize = 2_000;
const RANSAC_THRESHOLD: f64 = 3.0;

#[derive(Debug, Clone, Copy)]
pub struct WarpMiddleware {
    frame_scale: f32,
}

impl WarpMiddleware {
    pub fn new(frame_scale: f32) -> anyhow::Result<Self> {
        if !frame_scale.is_finite() || frame_scale <= 0.0 || frame_scale > 1.0 {
            return Err(anyhow::anyhow!(
                "Frame scale must be finite and in (0.0, 1.0]"
            ));
        }

        Ok(Self { frame_scale })
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let transform = frame
            .metadata
            .motion_matches
            .as_deref()
            .filter(|matches| matches.len() >= MIN_MATCHES)
            .and_then(|matches| estimate_motion(matches, self.frame_scale))
            .or_else(|| {
                frame
                    .metadata
                    .motion_vectors
                    .as_deref()
                    .filter(|vectors| vectors.len() >= MIN_MATCHES)
                    .and_then(|vectors| estimate_translation(vectors, self.frame_scale))
            });
        let Some(transform) = transform else {
            return Ok(frame);
        };

        let Some(inverse) = inverse_transform(transform) else {
            return Ok(frame);
        };
        let estimated_transform = [
            inverse.data[0] as f32,
            inverse.data[1] as f32,
            inverse.data[2] as f32,
            inverse.data[3] as f32,
            inverse.data[4] as f32,
            inverse.data[5] as f32,
        ];
        if estimated_transform.iter().any(|value| !value.is_finite()) {
            return Err(anyhow::anyhow!(
                "Estimated image transform exceeds supported limits"
            ));
        }
        let width = i32::try_from(frame.width)
            .map_err(|_| anyhow::anyhow!("Frame width exceeds purecv limits"))?;
        let height = i32::try_from(frame.height)
            .map_err(|_| anyhow::anyhow!("Frame height exceeds purecv limits"))?;
        if width <= 0 || height <= 0 {
            return Err(anyhow::anyhow!("Frame dimensions must be non-zero"));
        }

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
        let warped = warp_perspective(
            &image,
            &inverse,
            Size2i::new(width, height),
            InterpolationFlags::Linear,
            BorderTypes::Reflect,
            Scalar::all(0),
        )
        .map_err(|error| anyhow::anyhow!("purecv image warp failed: {error}"))?;

        frame.data = Arc::new(Bytes::from(warped.data));
        frame.metadata.estimated_transform = Some(estimated_transform);
        if let Some(points) = frame.metadata.feature_points.as_mut() {
            points.retain_mut(|(x, y)| {
                let transformed_x = f64::from(inverse.data[0]) * f64::from(*x)
                    + f64::from(inverse.data[1]) * f64::from(*y)
                    + inverse.data[2];
                let transformed_y = f64::from(inverse.data[3]) * f64::from(*x)
                    + f64::from(inverse.data[4]) * f64::from(*y)
                    + inverse.data[5];
                let rounded_x = transformed_x.round();
                let rounded_y = transformed_y.round();
                if !rounded_x.is_finite()
                    || !rounded_y.is_finite()
                    || rounded_x < 0.0
                    || rounded_y < 0.0
                    || rounded_x >= f64::from(width)
                    || rounded_y >= f64::from(height)
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
        Self::new(0.25).expect("valid")
    }
}

impl Middleware for WarpMiddleware {
    fn name(&self) -> &'static str {
        "warp"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        let frame_scale = self.frame_scale;
        tokio::task::spawn_blocking(move || WarpMiddleware { frame_scale }.process_frame(frame))
            .await?
            .map_err(Into::into)
    }
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
    use std::sync::Arc;

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
    async fn leaves_frames_without_enough_correspondences_unchanged() {
        let mut frame = Frame::new(Bytes::from(vec![80; 16 * 16 * 3]), 16, 16, VideoFormat::BGR);
        frame.metadata.motion_matches =
            Some(translated_matches(2.0, 1.0)[..MIN_MATCHES - 1].to_vec());
        let original = frame.data.clone();

        let processed = WarpMiddleware::new(1.0)
            .expect("valid frame scale")
            .process_async(frame)
            .await
            .expect("frame should pass through");

        assert!(Arc::ptr_eq(&processed.data, &original));
        assert!(processed.metadata.estimated_transform.is_none());
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
