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
    frame::{Frame, SimilarityTransform},
    middleware::{Middleware, ProcessResult},
    types::percent::Percent,
};

const BLUE_PIXEL: Rgba<u8> = image::Rgba([0, 0, 255, 255]);

#[derive(Debug, Clone)]
pub struct StabilizationMiddleware {
    debug: bool,
    crop_percent: Percent,
    smoothing_factor: Percent,
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    smoothed_transform: Option<SimilarityTransform>,
}

impl StabilizationMiddleware {
    pub fn new(debug: bool, crop_percent: Percent, smoothing_factor: Percent) -> Self {
        Self {
            debug,
            crop_percent,
            smoothing_factor,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let transform = match frame.metadata.similarity_transform.as_ref() {
            Some(v) => v,
            None => return Ok(frame),
        };

        let (crop_x, crop_y, crop_width, crop_height) = crop_bounds(
            frame.data.width(),
            frame.data.height(),
            f32::from(self.crop_percent),
        );

        let transform = self.smooth_transform(transform);

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
        .context("estimated stabilization transform is not invertible")?
        .invert();

        let mut warped = imageproc::geometric_transformations::warp(
            &frame.data,
            projection,
            Interpolation::Bilinear,
            Border::Constant(Rgba([0, 0, 0, 255])),
        );

        if self.debug {
            let crop_rect = Rect::at(crop_x as i32, crop_y as i32).of_size(crop_width, crop_height);
            draw_hollow_rect_mut(&mut warped, crop_rect, BLUE_PIXEL);
            frame.data = Arc::new(warped);
            return Ok(frame);
        }

        let cropped =
            image::imageops::crop_imm(&warped, crop_x, crop_y, crop_width, crop_height).to_image();

        frame.width = cropped.width() as usize;
        frame.height = cropped.height() as usize;
        frame.data = Arc::new(cropped);

        Ok(frame)
    }

    fn smooth_transform(&self, transform: &SimilarityTransform) -> SimilarityTransform {
        let mut state = self.state.lock().expect(POISONED_LOCK_MSG);
        let smoothed_transform = state.smoothed_transform.map_or(*transform, |previous| {
            let current_weight = f64::from(self.smoothing_factor);
            let previous_weight = 1.0 - current_weight;
            SimilarityTransform {
                a: previous.a * previous_weight + transform.a * current_weight,
                b: previous.b * previous_weight + transform.b * current_weight,
                tx: previous.tx * previous_weight + transform.tx * current_weight,
                ty: previous.ty * previous_weight + transform.ty * current_weight,
            }
        });

        state.smoothed_transform = Some(smoothed_transform);
        smoothed_transform
    }
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
