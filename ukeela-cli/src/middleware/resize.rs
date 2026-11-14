use std::sync::Arc;

use anyhow::Context;
use fast_image_resize::{ResizeAlg, ResizeOptions, Resizer};

use crate::{
    frame::{Frame, FrameData},
    middleware::{Middleware, ProcessResult},
    types::frame_scale::FrameScale,
};

#[derive(Debug, Clone, Copy)]
pub struct ResizeMiddleware {
    frame_scale: FrameScale,
    resize_alg: ResizeAlg,
}

impl ResizeMiddleware {
    pub fn new(frame_scale: FrameScale, resize_alg: ResizeAlg) -> Self {
        Self {
            frame_scale,
            resize_alg,
        }
    }

    #[inline]
    fn scale_dim(&self, dim: usize) -> usize {
        ((dim as f32 * f32::from(self.frame_scale)).round() as usize).max(1)
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let scale = f32::from(self.frame_scale);
        if scale >= 1.0 {
            return Ok(frame);
        }

        let scaled_width = self.scale_dim(frame.width);
        let scaled_height = self.scale_dim(frame.height);

        let dst_width = u32::try_from(scaled_width).unwrap_or(u32::MAX);
        let dst_height = u32::try_from(scaled_height).unwrap_or(u32::MAX);

        let mut resizer = Resizer::new(); // TODO: single instance
        let options = ResizeOptions::new().resize_alg(self.resize_alg);

        let resized = tokio::runtime::Handle::current()
            .spawn_blocking({
                let src_image = frame.data.clone();
                move || {
                    let mut dst_image = FrameData::new(dst_width, dst_height);
                    resizer
                        .resize(src_image.as_ref(), &mut dst_image, &options)
                        .context("image resizing")?;
                    Result::<_, anyhow::Error>::Ok(dst_image)
                }
            })
            .await??;

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

    async fn process_async(&self, frame: crate::frame::Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
    }
}
