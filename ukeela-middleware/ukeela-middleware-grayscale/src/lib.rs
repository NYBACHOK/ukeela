use std::sync::Arc;

use purecv::{
    core::Matrix,
    imgproc::{cvt_color_bgr_to_gray, cvt_color_bgra_to_gray, cvt_color_rgba_to_gray},
};
use ukeela_common::{Frame, Middleware, ProcessResult, VideoFormat};

#[derive(Debug, Clone, Copy)]
pub struct GrayscaleMiddleware {}

impl GrayscaleMiddleware {
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let channels = frame.format.channels();
        let frame_data = frame.frame_data_from_cpu();

        // Why cast and clone to purecv instead of image::imageproc?
        // Performance diff. I tested only on laptop, but difference is 2x times faster for this op
        // Ironically, but resizing is on oposite slower for purecv, but its cause `fast_image_resize`
        // do single job, but extremly well
        let color = Matrix::from_vec(
            frame_data.rows().count(),
            frame.width,
            channels,
            frame_data.to_vec(),
        );

        let grayscale = match frame.format {
            VideoFormat::BGR => cvt_color_bgr_to_gray(&color),
            VideoFormat::BGRA => cvt_color_bgra_to_gray(&color),
            VideoFormat::RGBA => cvt_color_rgba_to_gray(&color),
        }
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        frame.metadata.grayscale = Some(Arc::new(grayscale));

        Ok(frame)
    }
}

impl Default for GrayscaleMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for GrayscaleMiddleware {
    fn name(&self) -> &'static str {
        "grayscale"
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame).map_err(Into::into)
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        tokio::runtime::Handle::current()
            .spawn_blocking({
                let this = *self;

                move || this.process_frame(frame)
            })
            .await?
            .map_err(Into::into)
    }
}
