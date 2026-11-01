use std::str::FromStr;

use anyhow::{Context, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use gstreamer::{
    glib::object::Cast,
    prelude::{ElementExt, GstBinExt},
};

use crate::{
    frame::{Frame, VideoFormat},
    pipeline::{FramePipeline, gst::GstInputConfig, input::InputHandle},
};

pub struct GstInputReader {
    pipeline: gstreamer::Pipeline,
    appsink: gstreamer_app::AppSink,
    frame_pipeline: FramePipeline,
}

impl GstInputReader {
    pub fn try_new(cfg: GstInputConfig, frame_pipeline: FramePipeline) -> anyhow::Result<Self> {
        let pipeline = match cfg {
            GstInputConfig::Camera {
                camera_device,
                width,
                height,
                fps,
                output: _,
            } => {
                format!(
                    "v4l2src device={} extra-controls=\"s,auto_exposure=3,exposure_dynamic_framerate=0\" ! \
     image/jpeg,width={},height={},framerate={}/1 ! \
     jpegdec ! videoconvert ! video/x-raw,format=BGRA ! appsink name=sink sync=false emit-signals=true",
                    camera_device, width, height, fps
                )
            }
            GstInputConfig::Libcamera {
                output: _,
                width,
                height,
                fps,
            } => format!(
                "libcamerasrc ae-metering-mode=spot ! \
                 video/x-raw,format=BGR,width={width},height={height},framerate={fps}/1 ! \
                 appsink name=sink max-buffers=1 drop=true"
            ),
            GstInputConfig::File { path, output: _ } => {
                // uridecodebin automatically handles demuxing & decoding any video format
                let absolute_path =
                    std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::PathBuf::from(path));
                let uri = format!("file://{}", absolute_path.to_string_lossy());

                format!(
                    "uridecodebin uri=\"{}\" ! appsink name=sink sync=true emit-signals=true",
                    uri
                )
            }
            GstInputConfig::Custom {
                pipeline,
                output: _,
            } => pipeline,
        };

        tracing::info!(pipeline = %pipeline, "Opening GStreamer Ingress pipine");

        let pipeline = gstreamer::parse::launch(&pipeline)
            .context("Failed to create GStreamer input file pipeline")?
            .downcast::<gstreamer::Pipeline>()
            .unwrap();

        let appsink = pipeline
            .by_name("sink")
            .context("Appsink not found")?
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();

        Ok(Self {
            pipeline,
            appsink,
            frame_pipeline,
        })
    }

    pub fn start_input_thread(self, mut input_handle: InputHandle) {
        let mut shutdown_rx = input_handle.shutdown_rx.resubscribe();

        self.appsink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample({
                    let frame_pipeline = self.frame_pipeline.clone();

                    move |appsink| match process_sample(appsink, &mut input_handle) {
                        Ok(flow) => Ok(flow),
                        Err(error) => {
                            tracing::error!(%error, "Failed to process GStreamer input sample");
                            frame_pipeline.shutdown();

                            Err(gstreamer::FlowError::Error)
                        }
                    }
                })
                .build(),
        );

        if let Err(e) = self.pipeline.set_state(gstreamer::State::Playing) {
            tracing::error!(error = ?e, "Changing state of Ingress");

            self.frame_pipeline.shutdown();
            return;
        }

        tokio::spawn(async move {
            let bus = match self.pipeline.bus() {
                Some(bus) => bus,
                None => return,
            };

            tracing::info!("Started input gst pipeline");

            let mut stream = bus.stream();
            loop {
                let msg = tokio::select! {
                    _signal = shutdown_rx.recv() => {
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(msg) =>msg,
                            _ => break,
                        }
                    }
                };

                use gstreamer::MessageView;
                match msg.view() {
                    MessageView::Error(err) => {
                        tracing::error!(
                            "GStreamer error: {} ({})",
                            err.error(),
                            err.debug().unwrap_or_default()
                        );

                        break;
                    }
                    MessageView::Eos(..) => {
                        break;
                    }
                    _ => {}
                }
            }

            let _ = self.pipeline.set_state(gstreamer::State::Null);

            tracing::info!("Closed input gst pipeline");
        });
    }
}

fn process_sample(
    appsink: &gstreamer_app::AppSink,
    input_handle: &mut InputHandle,
) -> anyhow::Result<gstreamer::FlowSuccess> {
    let sample = appsink.pull_sample().context("Failed to pull sample")?;
    let caps = sample.caps().context("Sample has no caps")?;
    let structure = caps.structure(0).context("Sample caps have no structure")?;

    let width = structure
        .get::<i32>("width")
        .context("Sample caps have no valid width")? as u32;
    let height = structure
        .get::<i32>("height")
        .context("Sample caps have no valid height")? as u32;
    let format = structure
        .get::<String>("format")
        .context("Sample caps have no valid pixel format")?;
    let format = VideoFormat::from_str(&format).map_err(|e| anyhow::anyhow!(e))?;

    let buffer = sample.buffer().context("Sample has no buffer")?;
    let mapped = buffer
        .map_readable()
        .context("Failed to map sample buffer")?;
    let frame = Frame::new(Bytes::copy_from_slice(&mapped), width, height, format);

    input_handle
        .send_frame_nonblocking(frame)
        .map_err(|e| anyhow!("Input handle is closed. Error: {e}"))?;

    Ok(gstreamer::FlowSuccess::Ok)
}
