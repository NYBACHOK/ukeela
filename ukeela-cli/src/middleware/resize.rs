use purecv::core::Matrix;

use crate::{
    frame::Frame,
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

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let scaled_width = ((frame.width as f32 * f32::from(self.frame_scale)).round() as usize).max(1);
        let scaled_height = ((frame.height as f32 * f32::from(self.frame_scale)).round() as usize).max(1);

        // let image= Matrix::from_vec(rows, cols, frame.format.channels(), frame.data.to_vec());

        // let img = purecv::imgproc::resize(src, dst_size)

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
