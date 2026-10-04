use anyhow::Context;
use gstreamer::{
    glib::object::{Cast, ObjectExt},
    prelude::{ElementExt, GstBinExt},
};

use crate::pipeline::{gst::GstOutputConfig, output::OutputHandle};

pub struct GstOutputWriter {
    pipeline: gstreamer::Pipeline,
    appsrc: gstreamer_app::AppSrc,
    framerate: Option<i32>,
    fps_overlay: Option<gstreamer::Element>,
}

impl GstOutputWriter {
    pub fn try_new(cfg: GstOutputConfig, show_fps: bool) -> anyhow::Result<Self> {
        let overlay_stage = if show_fps {
            " ! textoverlay name=fps_overlay text=\"FPS: --\" halignment=left valignment=top"
        } else {
            ""
        };

        let (pipeline_str, framerate) = match cfg {
            GstOutputConfig::Display { width, height } => (
                format!(
                    "appsrc name=src caps=\"video/x-raw,width={},height={},format=BGRA\" ! \
                     videoconvert{} ! videoconvert ! autovideosink sync=false",
                    width, height, overlay_stage
                ),
                None,
            ),
            GstOutputConfig::File {
                path,
                width,
                height,
                fps,
            } => (
                format!(
                    "appsrc name=src caps=\"video/x-raw,width={},height={},format=BGRA,framerate={}/1\" ! \
                     videoconvert{} ! x264enc speed-preset=ultrafast tune=zerolatency ! \
                     h264parse ! mp4mux ! filesink location=\"{}\"",
                    width,
                    height,
                    fps,
                    overlay_stage,
                    path.to_string_lossy()
                ),
                Some(i32::try_from(fps).context("Output frame rate exceeds GStreamer limits")?),
            ),
            GstOutputConfig::Custom { pipeline } => (pipeline, None),
        };

        let pipeline = gstreamer::parse::launch(&pipeline_str)
            .context("Failed to create GStreamer output pipeline")?
            .downcast::<gstreamer::Pipeline>()
            .unwrap();

        let appsrc = pipeline
            .by_name("src")
            .context("Appsrc not found")?
            .downcast::<gstreamer_app::AppSrc>()
            .unwrap();

        appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);
        appsrc.set_format(gstreamer::Format::Time);

        let fps_overlay = if show_fps {
            Some(pipeline.by_name("fps_overlay").context(
                "FPS display requested, but output pipeline has no \
                 textoverlay element named fps_overlay",
            )?)
        } else {
            None
        };

        Ok(Self {
            pipeline,
            appsrc,
            framerate,
            fps_overlay,
        })
    }

    pub fn start_output_thread(self, mut output_handle: OutputHandle) {
        let Self {
            pipeline,
            appsrc,
            framerate,
            fps_overlay,
        } = self;

        pipeline.set_state(gstreamer::State::Playing).unwrap();

        tokio::spawn(async move {
            // Keep pipeline alive inside the async block for the lifetime of the stream
            let _pipeline = pipeline;

            let mut fps_window_start = std::time::Instant::now();
            let mut frames_in_window = 0u64;
            let mut fps = None;
            let mut last_caps: Option<(i32, i32)> = None;

            while let Some(frame) = output_handle.receive_frame().await {
                let (Ok(width), Ok(height)) =
                    (i32::try_from(frame.width), i32::try_from(frame.height))
                else {
                    tracing::error!("Frame dimensions exceed GStreamer limits");
                    continue;
                };

                // Update caps only when frame dimensions change
                if last_caps != Some((width, height)) {
                    let mut caps_builder = gstreamer::Caps::builder("video/x-raw")
                        .field("width", width)
                        .field("height", height)
                        .field("format", "BGRA");

                    if let Some(framerate) = framerate {
                        caps_builder =
                            caps_builder.field("framerate", gstreamer::Fraction::new(framerate, 1));
                    }

                    appsrc.set_caps(Some(&caps_builder.build()));
                    tracing::info!(width, height, "Output resolution changed");
                    last_caps = Some((width, height));
                }

                if let (Some(overlay), Some(fps_val)) = (&fps_overlay, fps) {
                    overlay.set_property("text", format!("FPS: {fps_val:.1}"));
                }

                // Convert Frame to GStreamer buffer
                let mut buffer = gstreamer::Buffer::with_size(frame.data.len())
                    .expect("Failed to create buffer");

                {
                    let buffer_mut = buffer.get_mut().unwrap();
                    let pts =
                        gstreamer::ClockTime::from_nseconds(frame.timestamp.as_nanos() as u64);
                    buffer_mut.set_pts(pts);

                    let mut map = buffer_mut.map_writable().unwrap();
                    map.copy_from_slice(&frame.data);
                }

                // Push buffer to pipeline
                if let Err(error) = appsrc.push_buffer(buffer) {
                    tracing::warn!("Failed to push frame to output sink: {error}");
                } else {
                    frames_in_window += 1;
                }

                let elapsed = fps_window_start.elapsed();
                if elapsed >= std::time::Duration::from_secs(1) {
                    fps = Some(frames_in_window as f64 / elapsed.as_secs_f64());
                    frames_in_window = 0;
                    fps_window_start = std::time::Instant::now();
                }
            }
        });
    }
}
