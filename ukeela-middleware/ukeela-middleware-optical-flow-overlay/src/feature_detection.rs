use super::*;

const GREEN_PIXEL: Rgba<u8> = image::Rgba([0, 255, 0, 255]);

#[derive(Debug, Clone, Copy)]
pub struct FeaturePointOverlayMiddleware {}

impl FeaturePointOverlayMiddleware {
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }

    pub fn process_frame(&self, mut frame: Frame) -> ProcessResult {
        let points = &frame.metadata.feature_points;

        if points.is_empty() {
            return Ok(frame);
        }

        let width = frame.width;
        let height = frame.height;

        // Unique ownership of the Arc buffer (Clone-on-Write)
        if Arc::strong_count(&frame.data) > 1 {
            let inner = (*frame.data).clone();
            frame.data = Arc::new(inner);
        }

        let raw_data =
            Arc::get_mut(&mut frame.data).expect("Frame data should be uniquely owned after clone");

        for point in points {
            let px = point.pt.x.round() as i32;
            let py = point.pt.y.round() as i32;

            // size is the diameter, so radius = size / 2.
            // Fallback to a minimum radius of 2 if size is 0 or unassigned.
            let radius = if point.size > 0.0 {
                ((point.size * 0.5).max(1.0)).round() as i32
            } else {
                2
            };

            let r_sq = radius * radius;
            let inner_r_sq = (radius - 1).max(0) * (radius - 1).max(0);

            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let dist_sq = dx * dx + dy * dy;

                    let is_ring = dist_sq <= r_sq && dist_sq >= inner_r_sq;
                    let is_center = dx.abs() + dy.abs() <= 1;

                    if is_ring || is_center {
                        let nx = px + dx;
                        let ny = py + dy;
                        if nx >= 0 && ny >= 0 {
                            let x = nx as usize;
                            let y = ny as usize;
                            if x < width && y < height {
                                raw_data.put_pixel(x as u32, y as u32, GREEN_PIXEL);
                            }
                        }
                    }
                }
            }
        }

        Ok(frame)
    }
}

impl Default for FeaturePointOverlayMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for FeaturePointOverlayMiddleware {
    fn name(&self) -> &'static str {
        "of_feature_detection_overlay"
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(self.process_frame(frame))
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame)
    }
}
