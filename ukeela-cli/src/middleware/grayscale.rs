use std::sync::{Arc, RwLock};

use purecv::{
    core::Matrix,
    imgproc::{cvt_color_bgr_to_gray, cvt_color_bgra_to_gray},
};

use crate::{
    frame::{Frame, VideoFormat},
    middleware::{Middleware, ProcessResult},
};

#[derive(Debug, Clone, Copy)]
pub struct GrayscaleMiddleware {}

impl GrayscaleMiddleware {
    pub fn new() -> Self {
        Self {}
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        if frame.width == 0 || frame.height == 0 {
            return Err(anyhow::anyhow!("frame dimensions must be non-zero"));
        }

        let channels = frame.format.channels();

        let row_bytes = frame
            .width
            .checked_mul(channels)
            .ok_or_else(|| anyhow::anyhow!("frame width dimension overflow"))?;
        let packed_len = row_bytes
            .checked_mul(frame.height)
            .ok_or_else(|| anyhow::anyhow!("frame height dimension overflow"))?;

        if frame.data.len() < packed_len {
            return Err(anyhow::anyhow!(
                "frame buffer is too small for its dimensions",
            ));
        }

        let row_stride = if frame.data.len().is_multiple_of(frame.height)
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
        let color = Matrix::from_vec(frame.height, frame.width, channels, packed);

        let grayscale = tokio::runtime::Handle::current()
            .spawn_blocking({
                move || match frame.format {
                    VideoFormat::BGR => cvt_color_bgr_to_gray(&color),
                    VideoFormat::BGRA => cvt_color_bgra_to_gray(&color),
                }
            })
            .await?
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        frame.metadata.grayscale = Some(Arc::new(RwLock::new(grayscale)));

        Ok(frame)
    }
}

impl Middleware for GrayscaleMiddleware {
    fn name(&self) -> &'static str {
        "grayscale"
    }

    async fn process_async(&self, frame: crate::frame::Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
    }
}
