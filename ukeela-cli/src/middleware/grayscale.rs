use std::sync::Arc;

use purecv::{
    core::Matrix,
    imgproc::{cvt_color_bgr_to_gray, cvt_color_bgra_to_gray, cvt_color_rgba_to_gray},
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
        let channels = frame.format.channels();

        // Why cast and clone to purecv instead of image::imageproc?
        // Performance diff. I tested only on laptop, but difference is 2x times faster for this op
        // Ironically, but resizing is on oposite slower for purecv, but its cause `fast_image_resize`
        // do single job, but extremly well
        let color = Matrix::from_vec(
            frame.data.rows().count(),
            frame.width,
            channels,
            frame.data.to_vec(),
        );

        let grayscale = tokio::runtime::Handle::current()
            .spawn_blocking({
                move || match frame.format {
                    VideoFormat::BGR => cvt_color_bgr_to_gray(&color),
                    VideoFormat::BGRA => cvt_color_bgra_to_gray(&color),
                    VideoFormat::RGBA => cvt_color_rgba_to_gray(&color),
                }
            })
            .await?
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        frame.metadata.grayscale = Some(Arc::new(grayscale));

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
