use std::sync::Arc;

use crate::{
    frame::{Frame, FrameData},
    middleware::{Middleware, ProcessResult},
};
use image::Rgba;

const RED_PIXEL: Rgba<u8> = image::Rgba([255, 0, 0, 255]);

#[derive(Debug, Clone, Copy)]
pub struct MotionMatchesOverlayMiddleware {}

impl MotionMatchesOverlayMiddleware {
    pub fn new() -> Self {
        Self {}
    }

    pub fn process_frame(&self, mut frame: Frame) -> ProcessResult {
        let matches = &frame.metadata.motion_matches;
        if matches.is_empty() {
            return Ok(frame);
        }

        if Arc::strong_count(&frame.data) > 1 {
            frame.data = Arc::new((*frame.data).clone());
        }

        let image =
            Arc::get_mut(&mut frame.data).expect("Frame data should be uniquely owned after clone");

        for motion_match in matches {
            let previous = motion_match.previous;
            let current = motion_match.current;

            draw_line(image, (previous.x, previous.y), (current.x, current.y));

            let dx = current.x - previous.x;
            let dy = current.y - previous.y;
            let length = dx.hypot(dy);
            if length > 0.0 && length.is_finite() {
                let unit_x = dx / length;
                let unit_y = dy / length;
                for side in [-1.0, 1.0] {
                    let arrow_end = (
                        current.x - unit_x * 5.0 + side * unit_y * 2.5,
                        current.y - unit_y * 5.0 - side * unit_x * 2.5,
                    );
                    draw_line(image, (current.x, current.y), arrow_end);
                }
            }
        }

        Ok(frame)
    }
}

fn draw_line(image: &mut FrameData, start: (f32, f32), end: (f32, f32)) {
    if !start.0.is_finite() || !start.1.is_finite() || !end.0.is_finite() || !end.1.is_finite() {
        return;
    }

    let (mut x, mut y) = (start.0.round() as i32, start.1.round() as i32);
    let (end_x, end_y) = (end.0.round() as i32, end.1.round() as i32);
    let dx = (end_x - x).abs();
    let sx = if x < end_x { 1 } else { -1 };
    let dy = -(end_y - y).abs();
    let sy = if y < end_y { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        if x >= 0 && y >= 0 && (x as u32) < image.width() && (y as u32) < image.height() {
            image.put_pixel(x as u32, y as u32, RED_PIXEL);
        }

        if x == end_x && y == end_y {
            break;
        }

        let doubled_error = 2 * error;
        if doubled_error >= dy {
            error += dy;
            x += sx;
        }
        if doubled_error <= dx {
            error += dx;
            y += sy;
        }
    }
}

impl Middleware for MotionMatchesOverlayMiddleware {
    fn name(&self) -> &'static str {
        "motion_matches_overlay"
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(self.process_frame(frame))
    }
}
