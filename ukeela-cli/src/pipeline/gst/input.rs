use anyhow::Context;
use bytes::Bytes;
use futures_util::StreamExt;
use gstreamer::{
    glib::object::Cast,
    prelude::{ElementExt, GstBinExt},
};

use crate::{
    frame::{Frame, VideoFormat},
    pipeline::{gst::GstInputConfig, input::InputHandle},
};

pub struct GstInputReader {
    pipeline: gstreamer::Pipeline,
    appsink: gstreamer_app::AppSink,
}

impl GstInputReader {
    pub fn try_new(cfg: GstInputConfig) -> anyhow::Result<Self> {
        let pipeline = match cfg {
            GstInputConfig::Camera {
                camera_device,
                width,
                height,
                fps,
                output: _,
            } => {
                format!(
                    "v4l2src device={} ! video/x-raw,width={},height={},framerate={}/1 ! \
             videoconvert ! appsink name=sink sync=false emit-signals=true",
                    camera_device, width, height, fps
                )
            }
            GstInputConfig::File { path, output: _ } => {
                // uridecodebin automatically handles demuxing & decoding any video format
                let absolute_path =
                    std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::PathBuf::from(path));
                let uri = format!("file://{}", absolute_path.to_string_lossy());

                format!(
                    "uridecodebin uri=\"{}\" ! videoconvert ! video/x-raw,format=RGB ! appsink name=sink sync=true emit-signals=true",
                    uri
                )
            }
            GstInputConfig::Custom {
                pipeline,
                output: _,
            } => pipeline,
        };

        let pipeline = gstreamer::parse::launch(&pipeline)
            .context("Failed to create GStreamer input file pipeline")?
            .downcast::<gstreamer::Pipeline>()
            .unwrap();

        let appsink = pipeline
            .by_name("sink")
            .context("Appsink not found")?
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();

        Ok(Self { pipeline, appsink })
    }

    pub fn start_input_thread(self, input_handle: InputHandle) {
        // Set up callback to push frames
        self.appsink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink
                        .pull_sample()
                        .map_err(|_| gstreamer::FlowError::Eos)?;

                    // Convert sample to Frame
                    let caps = sample.caps().ok_or(gstreamer::FlowError::Error)?;
                    let structure = caps.structure(0).ok_or(gstreamer::FlowError::Error)?;

                    let width = structure.get::<i32>("width").unwrap_or(1920) as u32;
                    let height = structure.get::<i32>("height").unwrap_or(1080) as u32;

                    let buffer = sample.buffer().ok_or(gstreamer::FlowError::Error)?;
                    let mapped = buffer
                        .map_readable()
                        .map_err(|_| gstreamer::FlowError::Error)?;

                    let data = Bytes::copy_from_slice(&mapped);
                    let frame = Frame::new(data, width, height, VideoFormat::RGB);

                    // Send to pipeline - non-blocking to avoid stalling GStreamer
                    match input_handle.send_frame_nonblocking(frame) {
                        Ok(true) => Ok(gstreamer::FlowSuccess::Ok),
                        Ok(false) => {
                            tracing::warn!("Backlog full, dropping frame");
                            Ok(gstreamer::FlowSuccess::Ok) // Drop silently
                        }
                        Err(_) => Err(gstreamer::FlowError::Eos),
                    }
                })
                .build(),
        );

        self.pipeline.set_state(gstreamer::State::Playing).unwrap();

        let pipeline = self.pipeline;
        tokio::spawn(async move {
            let bus = match pipeline.bus() {
                Some(bus) => bus,
                None => return,
            };

            let mut stream = bus.stream();
            while let Some(msg) = stream.next().await {
                use gstreamer::MessageView;
                match msg.view() {
                    MessageView::Error(err) => {
                        tracing::error!(
                            "GStreamer error: {} ({})",
                            err.error(),
                            err.debug().unwrap_or_default()
                        );
                        let _ = pipeline.set_state(gstreamer::State::Null);
                        break;
                    }
                    MessageView::Eos(..) => {
                        tracing::info!("End of stream");
                        let _ = pipeline.set_state(gstreamer::State::Null);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
}
