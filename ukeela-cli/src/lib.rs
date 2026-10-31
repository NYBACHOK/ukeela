use std::num::NonZero;

use crate::{
    middleware::{
        chain::ProcessingChain,
        feature_detection::{FeatureDetectionConfig, fast::FastFeatureDetectionMiddleware},
    },
    pipeline::{
        FramePipeline,
        gst::{GstInputConfig, GstInputReader, GstOutputConfig, GstOutputWriter, GstPixelFormat},
    },
};

pub mod frame;
pub mod middleware;
pub mod pipeline;

const POISONED_LOCK_MSG: &str = "poisoned lock";
pub const DEFAULT_CHANELLS_SIZE: usize = 32;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display, clap::ValueEnum,
)]
#[display(rename_all = "lowercase")]
pub enum FeaturePointsMethod {
    Fast,
    Ord,
    // #[display("open-cv")]
    // OpenCV,
    // OpenCVWithOpenCL,
    // Vulkan,
    // OpenCL,
}

#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Stabilization Flags")]
pub struct ProcessingChainFlags {
    /// Total percentage cropped from each frame dimension, split across opposite edges.
    #[arg(long, global = true, default_value_t = 25, value_parser = clap::value_parser!(u8).range(1..100), required = false)]
    pub crop_percent: u8,
    #[arg(long,global = true, default_value_t = FeaturePointsMethod::Fast, required = false)]
    pub features_method: FeaturePointsMethod,
}

pub async fn run(
    channel_size: usize,
    processing_fps: NonZero<u32>,
    show_fps: bool,
    input_cfg: GstInputConfig,
    output_cfg: GstOutputConfig,
    output_format: GstPixelFormat,
    flags: ProcessingChainFlags,
) -> Result<(), Box<dyn std::error::Error>> {
    gstreamer::init()?;

    let chain = build_processing_chain(flags)?;

    let (mut pipeline, input_handle, output_handle) =
        FramePipeline::new(chain, channel_size, processing_fps);
    let output_writer = GstOutputWriter::try_new(output_cfg, show_fps, output_format)?;
    let input_reader = GstInputReader::try_new(input_cfg)?;

    output_writer.start_output_thread(output_handle);
    input_reader.start_input_thread(input_handle);

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    loop {
        tokio::select! {
            ctrl_c_result = &mut ctrl_c => {
                match ctrl_c_result {
                    Ok(()) => tracing::warn!("Ctrl+C received, initiating graceful shutdown..."),
                    Err(err) => tracing::error!("Failed to listen for shutdown signal: {}", err),
                }
                break;
            }

            stats = pipeline.stats_rx.recv() => {
                match stats {
                    Ok(stats) => tracing::info!(
                        frames_processed = stats.frames_processed,
                        avg_fps = stats.avg_fps,
                        dropped_frames = stats.dropped_frames,
                        current_width = stats.current_width,
                        current_height = stats.current_height,
                        avg_processing_time_ms = stats.avg_processing_time_ms,
                        middleware_processing_times_ms = ?stats.middleware_processing_times_ms,
                        "Pipeline statistics for the last interval"
                    ),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("Skipped {} pipeline statistics updates", skipped);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}

fn build_processing_chain(
    ProcessingChainFlags {
        crop_percent,
        features_method,
    }: ProcessingChainFlags,
) -> Result<ProcessingChain, anyhow::Error> {
    let mut chain = ProcessingChain::new();
    let feature_frame_scale = f32::from(crop_percent) / 100.0;

    chain = chain.add_middleware(match features_method {
        FeaturePointsMethod::Fast => middleware::MiddlewareDisplatch::Fast(
            FastFeatureDetectionMiddleware::new(FeatureDetectionConfig {
                frame_scale: feature_frame_scale,
                num_features: 200,
                distance_to_border: 50,
                fast_threshold: 30,
                debug_features: false,
            })?,
        ),
        FeaturePointsMethod::Ord => todo!(),
    });

    Ok(chain)
}
