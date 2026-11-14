use std::sync::Arc;

use anyhow::Context;
use purecv::core::{Matrix, Size};

use crate::{
    frame::{Frame, FrameData},
    middleware::{Middleware, ProcessResult},
    types::frame_scale::FrameScale,
};

#[derive(Debug, Clone, Copy)]
pub struct ResizeMiddleware {
    frame_scale: FrameScale,
}

impl ResizeMiddleware {
    pub fn new(frame_scale: FrameScale) -> Self {
        Self { frame_scale }
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

        let image = Matrix::from_vec(
            frame.data.rows().count(),
            frame.width,
            frame.format.channels(),
            frame.data.to_vec(),
        );

        let img = tokio::runtime::Handle::current()
            .spawn_blocking({
                move || {
                    let img = purecv::imgproc::resize(
                        &image,
                        Size {
                            width: scaled_width,
                            height: scaled_height,
                        },
                    )
                    .context("image resizing")?;

                    Result::<_, anyhow::Error>::Ok(img)
                }
            })
            .await??;

        // Safer conversion from usize to u32 with error context
        let width_u32 = u32::try_from(scaled_width).context("scaled width exceeds u32 limits")?;
        let height_u32 =
            u32::try_from(scaled_height).context("scaled height exceeds u32 limits")?;

        let img = FrameData::from_raw(width_u32, height_u32, img.data)
            .ok_or_else(|| anyhow::anyhow!("image mapping failed"))?;

        frame.data = Arc::new(img);
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
