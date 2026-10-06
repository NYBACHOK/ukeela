use anyhow::Context;
use bytes::Bytes;
use gstreamer::{
    glib::object::{Cast, ObjectExt},
    prelude::{ElementExt, GstBinExt},
};

use crate::{
    frame::{Frame, VideoFormat},
    pipeline::{
        gst::{GstOutputConfig, GstPixelFormat},
        output::OutputHandle,
    },
};

pub struct GstOutputWriter {
    pipeline: gstreamer::Pipeline,
    appsrc: gstreamer_app::AppSrc,
    framerate: Option<i32>,
    fps_overlay: Option<gstreamer::Element>,
    output_format: GstPixelFormat,
}

impl GstOutputWriter {
    pub fn try_new(
        cfg: GstOutputConfig,
        show_fps: bool,
        output_format: GstPixelFormat,
    ) -> anyhow::Result<Self> {
        let format = output_format.as_gst_str();
        let overlay_stage = if show_fps {
            " ! textoverlay name=fps_overlay text=\"FPS: --\" halignment=left valignment=top"
        } else {
            ""
        };

        let (pipeline_str, framerate) = match cfg {
            GstOutputConfig::Display { width, height } => (
                format!(
                    "appsrc name=src caps=\"video/x-raw,width={},height={},format={}\" ! \
                     videoconvert{} ! videoconvert ! autovideosink sync=false",
                    width, height, format, overlay_stage
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
                    "appsrc name=src caps=\"video/x-raw,width={},height={},format={},framerate={}/1\" ! \
                     videoconvert{} ! x264enc speed-preset=ultrafast tune=zerolatency ! \
                     h264parse ! mp4mux ! filesink location=\"{}\"",
                    width,
                    height,
                    format,
                    fps,
                    overlay_stage,
                    path.to_string_lossy()
                ),
                Some(i32::try_from(fps).context("Output frame rate exceeds GStreamer limits")?),
            ),
            GstOutputConfig::Custom { pipeline } => (pipeline, None),
        };

        tracing::info!(pipeline = %pipeline_str, "Opening GStreamer Egress pipine");

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
            output_format,
        })
    }

    pub fn start_output_thread(self, mut output_handle: OutputHandle) {
        let Self {
            pipeline,
            appsrc,
            framerate,
            fps_overlay,
            output_format,
        } = self;

        let _ = pipeline
            .set_state(gstreamer::State::Playing)
            .inspect_err(|e| tracing::error!(error = ?e, "Changing state of Egress"));

        tokio::spawn(async move {
            // Keep pipeline alive inside the async block for the lifetime of the stream
            let _pipeline = pipeline;

            let mut fps_window_start = std::time::Instant::now();
            let mut frames_in_window = 0u64;
            let mut fps = None;
            let mut last_caps: Option<(i32, i32, &'static str)> = None;

            while let Some(frame) = output_handle.receive_frame().await {
                let (Ok(width), Ok(height)) =
                    (i32::try_from(frame.width), i32::try_from(frame.height))
                else {
                    tracing::error!("Frame dimensions exceed GStreamer limits");
                    continue;
                };

                // Update caps only when frame dimensions change
                let format = output_format.as_gst_str();
                if last_caps != Some((width, height, format)) {
                    let mut caps_builder = gstreamer::Caps::builder("video/x-raw")
                        .field("width", width)
                        .field("height", height)
                        .field("format", format);

                    if let Some(framerate) = framerate {
                        caps_builder =
                            caps_builder.field("framerate", gstreamer::Fraction::new(framerate, 1));
                    }

                    appsrc.set_caps(Some(&caps_builder.build()));
                    tracing::info!(width, height, "Output resolution changed");
                    last_caps = Some((width, height, format));
                }

                if let (Some(overlay), Some(fps_val)) = (&fps_overlay, fps) {
                    overlay.set_property("text", format!("FPS: {fps_val:.1}"));
                }

                let frame_data = match output_frame_data(&frame, output_format) {
                    Ok(data) => data,
                    Err(error) => {
                        tracing::error!(%error, frame_id = frame.id, "Failed to prepare output frame");
                        continue;
                    }
                };

                // Convert Frame to GStreamer buffer
                let mut buffer = gstreamer::Buffer::with_size(frame_data.len())
                    .expect("Failed to create buffer");

                {
                    let buffer_mut = buffer.get_mut().unwrap();
                    let pts =
                        gstreamer::ClockTime::from_nseconds(frame.timestamp.as_nanos() as u64);
                    buffer_mut.set_pts(pts);

                    let mut map = buffer_mut.map_writable().unwrap();
                    map.copy_from_slice(&frame_data);
                }

                // Push buffer to pipeline
                let push_result = appsrc.push_buffer(buffer);
                let (original_order, changed_order, arrival_time) = {
                    let metadata = frame.metadata.read().expect(crate::POISONED_LOCK_MSG);
                    (
                        metadata.original_order,
                        metadata.order,
                        metadata.arrival_time,
                    )
                };
                let processed_time = arrival_time.map(|arrival| arrival.elapsed());
                tracing::debug!(
                    frame_id = frame.id,
                    original_order = ?original_order,
                    changed_order = ?changed_order,
                    processed_time_ms = ?processed_time
                        .map(|duration| duration.as_secs_f64() * 1000.0),
                    egress_success = push_result.is_ok(),
                    "Frame processing completed from ingress to egress"
                );

                if let Err(error) = push_result {
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

fn output_frame_data(frame: &Frame, output_format: GstPixelFormat) -> anyhow::Result<Bytes> {
    let source_format = match frame.format {
        VideoFormat::BGR => GstPixelFormat::Bgr,
        VideoFormat::BGRA => GstPixelFormat::Bgra,
        _ => anyhow::bail!("unsupported input frame format for BGR/BGRA output"),
    };

    if source_format == output_format {
        return Ok((*frame.data).clone());
    }

    let width = usize::try_from(frame.width).context("Frame width exceeds platform limits")?;
    let height = usize::try_from(frame.height).context("Frame height exceeds platform limits")?;
    let pixels = width
        .checked_mul(height)
        .context("Frame dimensions overflow")?;
    let source_channels = match source_format {
        GstPixelFormat::Bgr => 3,
        GstPixelFormat::Bgra => 4,
    };
    let expected_len = pixels
        .checked_mul(source_channels)
        .context("Frame buffer size overflow")?;
    anyhow::ensure!(
        frame.data.len() == expected_len,
        "Frame buffer length does not match its dimensions and pixel format"
    );

    let output_channels = match output_format {
        GstPixelFormat::Bgr => 3,
        GstPixelFormat::Bgra => 4,
    };
    let mut output = Vec::with_capacity(
        pixels
            .checked_mul(output_channels)
            .context("Output frame buffer size overflow")?,
    );
    match (source_format, output_format) {
        (GstPixelFormat::Bgr, GstPixelFormat::Bgra) => {
            for pixel in frame.data.chunks_exact(3) {
                output.extend_from_slice(pixel);
                output.push(255);
            }
        }
        (GstPixelFormat::Bgra, GstPixelFormat::Bgr) => {
            for pixel in frame.data.chunks_exact(4) {
                output.extend_from_slice(&pixel[..3]);
            }
        }
        _ => unreachable!("matching input and output formats returned above"),
    }
    Ok(Bytes::from(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn converts_bgr_to_bgra() {
        let frame = Frame::new(
            Bytes::from_static(&[1, 2, 3, 4, 5, 6]),
            2,
            1,
            VideoFormat::BGR,
        );

        assert_eq!(
            output_frame_data(&frame, GstPixelFormat::Bgra).expect("valid BGR frame"),
            Bytes::from_static(&[1, 2, 3, 255, 4, 5, 6, 255])
        );
    }

    #[test]
    fn converts_bgra_to_bgr() {
        let frame = Frame::new(
            Bytes::from_static(&[1, 2, 3, 10, 4, 5, 6, 20]),
            2,
            1,
            VideoFormat::BGRA,
        );

        assert_eq!(
            output_frame_data(&frame, GstPixelFormat::Bgr).expect("valid BGRA frame"),
            Bytes::from_static(&[1, 2, 3, 4, 5, 6])
        );
    }
}
