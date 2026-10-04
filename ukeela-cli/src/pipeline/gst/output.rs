use anyhow::Context;
use gstreamer::{
    glib::object::Cast,
    prelude::{ElementExt, GstBinExt},
};

use crate::pipeline::{gst::GstOutputConfig, output::OutputHandle};

pub struct GstOutputWriter {
    pipeline: gstreamer::Pipeline,
    appsrc: gstreamer_app::AppSrc,
}

impl GstOutputWriter {
    pub fn try_new(cfg: GstOutputConfig) -> anyhow::Result<Self> {
        let pipeline = match cfg {
            GstOutputConfig::Display { width, height } => format!(
                "appsrc name=src caps=\"video/x-raw,width={},height={},format=BGRA\" ! \
             videoconvert ! autovideosink sync=false",
                width, height
            ),
            GstOutputConfig::File {
                path,
                width,
                height,
                fps,
            } => format!(
                "appsrc name=src caps=\"video/x-raw,width={},height={},format=BGRA,framerate={}/1\" ! \
             videoconvert ! x264enc speed-preset=ultrafast tune=zerolatency ! \
             h264parse ! mp4mux ! filesink location=\"{}\"",
                width,
                height,
                fps,
                path.to_string_lossy()
            ),
            GstOutputConfig::Custom { pipeline } => pipeline,
        };

        let pipeline = gstreamer::parse::launch(&pipeline)
            .context("Failed to create GStreamer input file pipeline")?
            .downcast::<gstreamer::Pipeline>()
            .unwrap();

        let appsrc = pipeline
            .by_name("src")
            .context("Appsrc not found")?
            .downcast::<gstreamer_app::AppSrc>()
            .unwrap();

        appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);
        appsrc.set_format(gstreamer::Format::Time);

        Ok(Self { pipeline, appsrc })
    }

    pub fn start_output_thread(self, mut output_handle: OutputHandle) {
        let appsrc = self.appsrc.clone();

        self.pipeline.set_state(gstreamer::State::Playing).unwrap();

        tokio::spawn(async move {
            while let Some(frame) = output_handle.receive_frame().await {
                // Convert Frame to GStreamer buffer
                let mut buffer = gstreamer::Buffer::with_size(frame.data.len())
                    .expect("Failed to create buffer");

                {
                    let buffer_mut = buffer.get_mut().unwrap();

                    // Set timestamp
                    let pts =
                        gstreamer::ClockTime::from_nseconds(frame.timestamp.as_nanos() as u64);
                    buffer_mut.set_pts(pts);

                    // Write frame data to buffer
                    let mut map = buffer_mut.map_writable().unwrap();
                    map.copy_from_slice(&frame.data);
                }

                // Push to pipeline
                if let Err(_) = appsrc.push_buffer(buffer) {
                    tracing::warn!("Failed to push frame to output sink");
                }
            }
        });
    }
}
