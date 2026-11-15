use std::sync::{Arc, Mutex};

use anyhow::Context;
use image::Rgba;
use imageproc::{
    drawing::draw_hollow_rect_mut,
    geometric_transformations::{Border, Interpolation, Projection},
    rect::Rect,
};

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, FrameData, SimilarityTransform},
    middleware::{Middleware, ProcessResult},
    types::percent::Percent,
};

const BLUE_PIXEL: Rgba<u8> = image::Rgba([0, 0, 255, 255]);
const TRANSFORM_EPSILON: f64 = 1e-12;

#[derive(Debug, Clone)]
pub struct StabilizationMiddleware {
    debug: bool,
    crop_percent: Percent,
    smoothing_factor: Percent,
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    camera_trajectory: Option<SimilarityTransform>,
    smoothed_trajectory: Option<SimilarityTransform>,
}

impl StabilizationMiddleware {
    #[must_use]
    pub fn new(debug: bool, crop_percent: Percent, smoothing_factor: Percent) -> Self {
        Self {
            debug,
            crop_percent,
            smoothing_factor,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let correction = match frame.metadata.similarity_transform.as_ref() {
            Some(relative) => Some(
                self.update_trajectory(relative)
                    .context("could not update stabilization trajectory")?,
            ),
            None => None,
        };

        let (crop_x, crop_y, crop_width, crop_height) = crop_bounds(
            frame.data.width(),
            frame.data.height(),
            f32::from(self.crop_percent),
        );

        if self.debug {
            let mut image = match correction {
                Some(transform) if !is_identity(transform) => warp(&frame.data, transform)?,
                _ => frame.data.as_ref().clone(),
            };
            let crop_rect = Rect::at(crop_x as i32, crop_y as i32).of_size(crop_width, crop_height);
            draw_hollow_rect_mut(&mut image, crop_rect, BLUE_PIXEL);
            frame.data = Arc::new(image);
            return Ok(frame);
        }

        let cropped = match correction {
            Some(transform) if !is_identity(transform) => {
                let warped = warp(&frame.data, transform)?;
                image::imageops::crop_imm(&warped, crop_x, crop_y, crop_width, crop_height)
                    .to_image()
            }
            _ => image::imageops::crop_imm(
                frame.data.as_ref(),
                crop_x,
                crop_y,
                crop_width,
                crop_height,
            )
            .to_image(),
        };

        frame.width = cropped.width() as usize;
        frame.height = cropped.height() as usize;
        frame.data = Arc::new(cropped);

        Ok(frame)
    }

    fn update_trajectory(
        &self,
        relative_motion: &SimilarityTransform,
    ) -> Option<SimilarityTransform> {
        if !is_valid_transform(*relative_motion) {
            return None;
        }

        let mut state = self.state.lock().expect(POISONED_LOCK_MSG);
        let actual = compose(
            *relative_motion,
            state.camera_trajectory.unwrap_or_else(identity),
        );
        if !is_valid_transform(actual) {
            return None;
        }

        let smoothed = match state.smoothed_trajectory {
            Some(previous) => {
                interpolate_similarity(previous, actual, f64::from(self.smoothing_factor))?
            }
            None => actual,
        };
        let correction = compose(smoothed, inverse(actual)?);
        if !is_valid_transform(correction) {
            return None;
        }

        state.camera_trajectory = Some(actual);
        state.smoothed_trajectory = Some(smoothed);
        Some(correction)
    }
}

fn warp(image: &FrameData, transform: SimilarityTransform) -> anyhow::Result<FrameData> {
    let projection = Projection::from_matrix([
        transform.a as f32,
        -transform.b as f32,
        transform.tx as f32,
        transform.b as f32,
        transform.a as f32,
        transform.ty as f32,
        0.0,
        0.0,
        1.0,
    ])
    .context("stabilization correction is not invertible")?;

    Ok(imageproc::geometric_transformations::warp(
        image,
        projection,
        Interpolation::Bilinear,
        Border::Constant(Rgba([0, 0, 0, 255])),
    ))
}

fn identity() -> SimilarityTransform {
    SimilarityTransform {
        a: 1.0,
        b: 0.0,
        tx: 0.0,
        ty: 0.0,
    }
}

// Returns outer(inner(point)); relative motion therefore composes on the left.
fn compose(outer: SimilarityTransform, inner: SimilarityTransform) -> SimilarityTransform {
    SimilarityTransform {
        a: outer.a * inner.a - outer.b * inner.b,
        b: outer.b * inner.a + outer.a * inner.b,
        tx: outer.a * inner.tx - outer.b * inner.ty + outer.tx,
        ty: outer.b * inner.tx + outer.a * inner.ty + outer.ty,
    }
}

fn inverse(transform: SimilarityTransform) -> Option<SimilarityTransform> {
    let determinant = transform.a * transform.a + transform.b * transform.b;
    if !determinant.is_finite() || determinant <= TRANSFORM_EPSILON {
        return None;
    }

    Some(SimilarityTransform {
        a: transform.a / determinant,
        b: -transform.b / determinant,
        tx: -(transform.a * transform.tx + transform.b * transform.ty) / determinant,
        ty: (transform.b * transform.tx - transform.a * transform.ty) / determinant,
    })
}

fn interpolate_similarity(
    previous: SimilarityTransform,
    current: SimilarityTransform,
    current_weight: f64,
) -> Option<SimilarityTransform> {
    if !is_valid_transform(previous) || !is_valid_transform(current) {
        return None;
    }

    // Causal smoothing uses past state only, so it adds no lookahead latency.
    let previous_scale = previous.a.hypot(previous.b);
    let current_scale = current.a.hypot(current.b);
    let previous_angle = previous.b.atan2(previous.a);
    let current_angle = current.b.atan2(current.a);
    let angle_delta = (current_angle - previous_angle + std::f64::consts::PI)
        .rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    let weight = current_weight.clamp(0.0, 1.0);
    let angle = previous_angle + angle_delta * weight;
    let scale = (previous_scale.ln() * (1.0 - weight) + current_scale.ln() * weight).exp();

    let transform = SimilarityTransform {
        a: scale * angle.cos(),
        b: scale * angle.sin(),
        tx: previous.tx * (1.0 - weight) + current.tx * weight,
        ty: previous.ty * (1.0 - weight) + current.ty * weight,
    };

    is_valid_transform(transform).then_some(transform)
}

fn is_valid_transform(transform: SimilarityTransform) -> bool {
    [transform.a, transform.b, transform.tx, transform.ty]
        .into_iter()
        .all(f64::is_finite)
        && [transform.a, transform.b, transform.tx, transform.ty]
            .into_iter()
            .map(|value| value as f32)
            .all(f32::is_finite)
        && transform.a * transform.a + transform.b * transform.b > TRANSFORM_EPSILON
}

fn is_identity(transform: SimilarityTransform) -> bool {
    (transform.a - 1.0).abs() <= TRANSFORM_EPSILON
        && transform.b.abs() <= TRANSFORM_EPSILON
        && transform.tx.abs() <= TRANSFORM_EPSILON
        && transform.ty.abs() <= TRANSFORM_EPSILON
}

#[inline]
fn crop_bounds(width: u32, height: u32, crop_percent: f32) -> (u32, u32, u32, u32) {
    let margin_x = ((width as f32 * crop_percent).round() as u32).min(width.saturating_sub(1) / 2);
    let margin_y =
        ((height as f32 * crop_percent).round() as u32).min(height.saturating_sub(1) / 2);

    (
        margin_x,
        margin_y,
        width - 2 * margin_x,
        height - 2 * margin_y,
    )
}

impl Middleware for StabilizationMiddleware {
    fn name(&self) -> &'static str {
        "stabilization"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        let middleware = self.clone();
        let frame = tokio::task::spawn_blocking(move || middleware.process_frame(frame)).await??;
        Ok(frame)
    }
}
