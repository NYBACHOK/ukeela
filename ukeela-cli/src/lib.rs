use std::num::NonZero;

use crate::{
    middleware::{
        chain::ProcessingChain,
        ema::{EmaMiddleware, config::EmaConfig},
        feature_points::FeaturePointsMiddleware,
        stabilization::{StabilizationConfig, StabilizationMiddleware},
    },
    pipeline::{
        FramePipeline,
        gst::{GstInputConfig, GstInputReader, GstOutputConfig, GstOutputWriter},
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
pub enum FeaturePointsBackend {
    Cpu,
    OpenCV,
    // Vulkan,
    // OpenCL,
}

#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Stabilization Flags")]
pub struct ProcessingChainFlags {
    /// Total percentage cropped from each frame dimension, split across opposite edges.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..100), required = false)]
    pub crop_percent: Option<u8>,
    /// Use ema with reading of movement vectors if they available
    #[arg(long, default_value_t = false, required = false)]
    pub use_ema: bool,
    #[arg(long, default_value_t = FeaturePointsBackend::OpenCV, required = false)]
    pub features_backend: FeaturePointsBackend,
}

pub async fn run(
    channel_size: usize,
    processing_fps: NonZero<u32>,
    show_fps: bool,
    input_cfg: GstInputConfig,
    output_cfg: GstOutputConfig,
    flags: ProcessingChainFlags,
) -> Result<(), Box<dyn std::error::Error>> {
    gstreamer::init()?;

    let chain = build_processing_chain(flags)?;

    let (mut pipeline, input_handle, output_handle) =
        FramePipeline::new(chain, channel_size, processing_fps);
    let output_writer = GstOutputWriter::try_new(output_cfg, show_fps)?;
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
        use_ema,
        features_backend,
    }: ProcessingChainFlags,
) -> Result<ProcessingChain, anyhow::Error> {
    let mut chain = ProcessingChain::new();

    if use_ema {
        chain = chain.add_middleware(middleware::MiddlewareDisplatch::Ema(EmaMiddleware::new(
            EmaConfig::default(),
        )));
    }

    chain = chain.add_middleware(match features_backend {
        FeaturePointsBackend::Cpu => {
            middleware::MiddlewareDisplatch::FeaturePoints(FeaturePointsMiddleware)
        }
        FeaturePointsBackend::OpenCV => todo!(),
        // FeaturePointsBackend::Vulkan => todo!(),
        // FeaturePointsBackend::OpenCL => todo!(),
    });

    chain = chain.add_middleware(middleware::MiddlewareDisplatch::Stabilization(
        StabilizationMiddleware::new(StabilizationConfig {
            crop_percent,
            ..StabilizationConfig::default()
        })
        .map_err(anyhow::Error::msg)?,
    ));

    Ok(chain)
}
