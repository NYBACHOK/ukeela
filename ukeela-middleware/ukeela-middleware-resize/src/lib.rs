use std::sync::{Arc, Mutex};

use anyhow::Context;
use fast_image_resize::{ResizeAlg, ResizeOptions, Resizer};
use ukeela_common::{
    Frame, FrameData, Middleware, ProcessResult, extensions::lock::AcquireMutexLock, types::Percent,
};

#[derive(Debug, Clone)]
pub struct ResizeMiddleware {
    frame_scale: Percent,
    resize_alg: ResizeAlg,
    resizer: Arc<Mutex<Resizer>>,
}

impl ResizeMiddleware {
    #[must_use]
    pub fn new(frame_scale: Percent, resize_alg: ResizeAlg) -> Self {
        Self {
            frame_scale,
            resize_alg,
            resizer: Arc::new(Mutex::new(Resizer::new())),
        }
    }

    #[inline]
    fn scale_dim(&self, dim: usize) -> usize {
        ((dim as f32 * f32::from(self.frame_scale)).round() as usize).max(1)
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let scale = f32::from(self.frame_scale);
        if scale >= 1.0 {
            return Ok(frame);
        }

        let scaled_width = self.scale_dim(frame.width);
        let scaled_height = self.scale_dim(frame.height);

        let dst_width = u32::try_from(scaled_width).unwrap_or(u32::MAX);
        let dst_height = u32::try_from(scaled_height).unwrap_or(u32::MAX);

        let options = ResizeOptions::new().resize_alg(self.resize_alg);

        let resized = {
            let mut dst_image = FrameData::new(dst_width, dst_height);
            self.resizer
                .acquire_lock()
                .resize(frame.data.as_ref(), &mut dst_image, &options)
                .context("image resizing")?;

            dst_image
        };

        frame.data = Arc::new(resized);
        frame.width = scaled_width;
        frame.height = scaled_height;

        Ok(frame)
    }
}

impl Middleware for ResizeMiddleware {
    fn name(&self) -> &'static str {
        "resize"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        tokio::runtime::Handle::current()
            .spawn_blocking({
                let this = self.clone();

                move || this.process_frame(frame)
            })
            .await?
            .map_err(Into::into)
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame)
    }
}
