use std::{cmp::Ordering, io};

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, VideoFormat},
    middleware::{Middleware, ProcessResult},
};

const MAX_FEATURE_POINTS: usize = 200;
const MIN_FEATURE_DISTANCE: u32 = 8;

#[derive(Debug, Default)]
pub struct FeaturePointsMiddleware;

impl Middleware for FeaturePointsMiddleware {
    fn name(&self) -> &'static str {
        "cpu_non_accelerated_feature_points"
    }

    fn estimated_latency(&self) -> f32 {
        2.0
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(process_frame(frame))
    }
}

fn process_frame(frame: Frame) -> ProcessResult {
    let should_detect = {
        let metadata = frame.metadata.read().expect(POISONED_LOCK_MSG);
        metadata
            .estimated_transform
            .is_none_or(|transform| transform.iter().all(|value| *value == 0.0))
    };

    if !should_detect {
        return Ok(frame);
    }

    let grayscale = grayscale_pixels(&frame)?;
    let points = detect_corners(&grayscale, frame.width as usize, frame.height as usize);
    frame
        .metadata
        .write()
        .expect(POISONED_LOCK_MSG)
        .feature_points = Some(points);

    Ok(frame)
}

fn grayscale_pixels(frame: &Frame) -> Result<Vec<f32>, io::Error> {
    let width = usize::try_from(frame.width)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame width is too large"))?;
    let height = usize::try_from(frame.height)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame height is too large"))?;
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "frame dimensions overflow"))?;

    match frame.format {
        VideoFormat::I420 | VideoFormat::NV12 => {
            if frame.data.len() < pixel_count {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "frame buffer is too small for its dimensions",
                ));
            }

            Ok(frame.data[..pixel_count]
                .iter()
                .map(|value| f32::from(*value))
                .collect())
        }
        VideoFormat::RGB | VideoFormat::RGBA | VideoFormat::BGRA => {
            let bytes_per_pixel = match frame.format {
                VideoFormat::RGB => 3,
                VideoFormat::RGBA | VideoFormat::BGRA => 4,
                VideoFormat::I420 | VideoFormat::NV12 => unreachable!(),
            };
            let row_bytes = width.checked_mul(bytes_per_pixel).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "frame dimensions overflow")
            })?;
            let packed_len = row_bytes.checked_mul(height).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "frame dimensions overflow")
            })?;

            if frame.data.len() < packed_len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "frame buffer is too small for its dimensions",
                ));
            }

            let row_stride = if height > 0
                && frame.data.len() % height == 0
                && frame.data.len() / height >= row_bytes
            {
                frame.data.len() / height
            } else {
                row_bytes
            };
            let (red, green, blue) = if matches!(frame.format, VideoFormat::BGRA) {
                (2, 1, 0)
            } else {
                (0, 1, 2)
            };

            let mut grayscale = Vec::with_capacity(pixel_count);
            for y in 0..height {
                let row_start = y * row_stride;
                for x in 0..width {
                    let pixel_start = row_start + x * bytes_per_pixel;
                    let r = f32::from(frame.data[pixel_start + red]);
                    let g = f32::from(frame.data[pixel_start + green]);
                    let b = f32::from(frame.data[pixel_start + blue]);
                    grayscale.push(0.299 * r + 0.587 * g + 0.114 * b);
                }
            }

            Ok(grayscale)
        }
    }
}

fn detect_corners(grayscale: &[f32], width: usize, height: usize) -> Vec<(u32, u32)> {
    if width < 5 || height < 5 {
        return Vec::new();
    }

    let mut responses = vec![0.0_f32; width * height];
    let mut maximum_response = 0.0_f32;

    for y in 2..height - 2 {
        for x in 2..width - 2 {
            let mut xx = 0.0;
            let mut yy = 0.0;
            let mut xy = 0.0;

            for window_y in y - 1..=y + 1 {
                for window_x in x - 1..=x + 1 {
                    let index = window_y * width + window_x;
                    let dx = grayscale[index + 1] - grayscale[index - 1];
                    let dy = grayscale[index + width] - grayscale[index - width];
                    xx += dx * dx;
                    yy += dy * dy;
                    xy += dx * dy;
                }
            }

            let trace = xx + yy;
            let response = xx * yy - xy * xy - 0.04 * trace * trace;
            if response > 0.0 {
                responses[y * width + x] = response;
                maximum_response = maximum_response.max(response);
            }
        }
    }

    if maximum_response == 0.0 {
        return Vec::new();
    }

    let threshold = maximum_response * 0.01;
    let mut candidates = Vec::new();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let response = responses[y * width + x];
            if response < threshold {
                continue;
            }
            let is_local_maximum = (y - 1..=y + 1).all(|neighbor_y| {
                (x - 1..=x + 1).all(|neighbor_x| {
                    neighbor_y == y && neighbor_x == x
                        || responses[neighbor_y * width + neighbor_x] <= response
                })
            });
            if is_local_maximum {
                candidates.push((response, x, y));
            }
        }
    }

    candidates.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(Ordering::Equal));
    let min_distance_squared = MIN_FEATURE_DISTANCE * MIN_FEATURE_DISTANCE;
    let mut points: Vec<(u32, u32)> = Vec::with_capacity(MAX_FEATURE_POINTS.min(candidates.len()));
    for (_, x, y) in candidates {
        let (Ok(point_x), Ok(point_y)) = (u32::try_from(x), u32::try_from(y)) else {
            continue;
        };
        let sufficiently_far = points.iter().all(|(existing_x, existing_y)| {
            let dx = u32::abs_diff(point_x, *existing_x);
            let dy = u32::abs_diff(point_y, *existing_y);
            dx * dx + dy * dy >= min_distance_squared
        });
        if sufficiently_far {
            points.push((point_x, point_y));
            if points.len() == MAX_FEATURE_POINTS {
                break;
            }
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::MAX_FEATURE_POINTS;
    use crate::{
        frame::{Frame, VideoFormat},
        middleware::{Middleware, feature_points::cpu::FeaturePointsMiddleware},
    };

    fn checkerboard_frame(transform: Option<[f32; 6]>) -> Frame {
        let width = 32;
        let height = 32;
        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let value = if (x / 8 + y / 8) % 2 == 0 { 0 } else { 255 };
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }

        let frame = Frame::new(
            Bytes::from(pixels),
            u32::try_from(width).expect("test image width fits in u32"),
            u32::try_from(height).expect("test image height fits in u32"),
            VideoFormat::BGRA,
        );
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .estimated_transform = transform;
        frame
    }

    #[tokio::test]
    async fn detects_points_when_transform_is_missing_or_zero() {
        let middleware = FeaturePointsMiddleware;

        for transform in [None, Some([0.0; 6])] {
            let frame = middleware
                .process_async(checkerboard_frame(transform))
                .await
                .expect("frame processing should succeed");
            let metadata = frame
                .metadata
                .read()
                .expect("metadata lock should not be poisoned");
            let points = metadata
                .feature_points
                .as_ref()
                .expect("feature points should be set");

            assert!(!points.is_empty());
            assert!(points.len() <= MAX_FEATURE_POINTS);
        }
    }

    #[tokio::test]
    async fn leaves_existing_points_untouched_when_transform_is_present() {
        let middleware = FeaturePointsMiddleware;
        let frame = checkerboard_frame(Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
        frame
            .metadata
            .write()
            .expect("metadata lock should not be poisoned")
            .feature_points = Some(vec![(3, 4)]);

        let frame = middleware
            .process_async(frame)
            .await
            .expect("frame processing should succeed");
        let metadata = frame
            .metadata
            .read()
            .expect("metadata lock should not be poisoned");

        assert_eq!(metadata.feature_points.as_deref(), Some(&[(3, 4)][..]));
    }
}
