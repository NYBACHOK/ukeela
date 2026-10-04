use crate::middleware::ema::config::{EmaConfig, MotionVector};

#[derive(Debug)]
pub struct EmaState {
    pub config: EmaConfig,

    // Cumulative trajectory (where the camera actually is)
    cum_dx: f32,
    cum_dy: f32,

    // Smoothed trajectory (where the camera should be)
    smooth_x: f32,
    smooth_y: f32,

    frame_count: u64,
}

impl EmaState {
    pub fn new(config: EmaConfig) -> Self {
        Self {
            frame_count: 0,
            config,
            cum_dx: 0.0,
            cum_dy: 0.0,
            smooth_x: 0.0,
            smooth_y: 0.0,
        }
    }

    pub fn update_single(&mut self, raw: &MotionVector) -> (f32, f32) {
        self.frame_count += 1;

        let dx = raw.dx.clamp(-self.config.max_delta, self.config.max_delta);
        let dy = raw.dy.clamp(-self.config.max_delta, self.config.max_delta);

        // Accumulate total camera displacement
        self.cum_dx += dx;
        self.cum_dy += dy;

        let alpha = if self.frame_count <= self.config.warmup_frames as u64 {
            self.frame_count as f32 / self.config.warmup_frames as f32 * self.config.alpha
        } else {
            self.config.alpha
        };

        // Smooth the accumulated trajectory
        self.smooth_x = alpha * self.cum_dx + (1.0 - alpha) * self.smooth_x;
        self.smooth_y = alpha * self.cum_dy + (1.0 - alpha) * self.smooth_y;

        self.get_offset()
    }

    pub fn get_offset(&self) -> (f32, f32) {
        let offset_x = self.smooth_x - self.cum_dx;
        let offset_y = self.smooth_y - self.cum_dy;

        (offset_x, offset_y)
    }
}
