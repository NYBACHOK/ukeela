pub mod types;
use std::num::NonZero;

use crate::{
    middleware::{
        chain::ProcessingChain,
        feature_detection::{
            FeatureDetectionConfig, fast::FastFeatureDetectionMiddleware,
            orb::OrbFeatureDetectionMiddleware, overlay::FeaturePointOverlayMiddleware,
        },
        grayscale::GrayscaleMiddleware,
        motion::{
            DEFAULT_MIN_MATCHES, estimation::MotionEstimationMiddleware,
            matches::MotionMatchesMiddleware, overlay::MotionMatchesOverlayMiddleware,
        },
        resize::ResizeMiddleware,
        stabilization::StabilizationMiddleware,
    },
    pipeline::{
        FramePipeline,
        gst::{GstInputConfig, GstInputReader, GstOutputConfig, GstOutputWriter},
    },
    types::percent::Percent,
};

pub mod frame;
pub mod middleware;
pub mod pipeline;

pub const POISONED_LOCK_MSG: &str = "poisoned lock";

pub const DEFAULT_CHANELLS_SIZE: usize = 32;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display, clap::ValueEnum,
)]
#[display(rename_all = "lowercase")]
pub enum FeaturePointsMethod {
    Fast,
    Orb,
}

/// Type of filter used for image convolution.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, derive_more::Display, clap::ValueEnum)]
#[display(rename_all = "lowercase")]
pub enum FilterType {
    Box,
    #[default]
    Bilinear,
    Hamming,
    CatmullRom,
    Mitchell,
    Gaussian,
    Lanczos3,
}

impl From<FilterType> for fast_image_resize::FilterType {
    fn from(value: FilterType) -> Self {
        match value {
            FilterType::Box => Self::Box,
            FilterType::Bilinear => Self::Bilinear,
            FilterType::Hamming => Self::Hamming,
            FilterType::CatmullRom => Self::CatmullRom,
            FilterType::Mitchell => Self::Mitchell,
            FilterType::Gaussian => Self::Gaussian,
            FilterType::Lanczos3 => Self::Lanczos3,
        }
    }
}

#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Processing Options")]
pub struct ProcessingChainFlags {
    /// Scale original image to specific percent of it's original size
    #[arg(long, global = true, default_value_t = 25, value_parser = clap::value_parser!(u8).range(1..101), required = false)]
    pub scale: u8,
    /// Crop this percentage from each edge for stabilization
    #[arg(long, global = true, default_value_t = 5, value_parser = clap::value_parser!(u8).range(1..51), required = false)]
    pub crop: u8,
    /// Weight of the latest motion estimate in smoothing (percent; higher is more responsive)
    #[arg(long, global = true, default_value_t = 50, value_parser = clap::value_parser!(u8).range(1..101), required = false)]
    pub smoothing_factor: u8,
    /// Draw the stabilization crop boundary without cropping
    #[arg(long, global = true, default_value_t = false, required = false)]
    pub display_stabilization: bool,
    #[arg(long,global = true, default_value_t = FeaturePointsMethod::Fast, required = false)]
    pub features_method: FeaturePointsMethod,
    #[arg(long,global = true, default_value_t = FilterType::Bilinear, required = false)]
    pub filter_type: FilterType,
    /// Show found features on image
    #[arg(long, global = true, default_value_t = false, required = false)]
    pub display_features: bool,
    /// Show optical-flow motion vectors on the image
    #[arg(long, global = true, default_value_t = false, required = false)]
    pub display_motion_vectors: bool,
    #[arg(long, global = true, default_value_t = NonZero::new(100).unwrap(), required = false)]
    pub num_features: NonZero<usize>,
    #[arg(long, global = true, default_value_t = 50, required = false)]
    pub distance_to_border: u32,
    #[arg(long, global = true, default_value_t = 30, required = false)]
    pub fast_threshold: u8,
    #[arg(long, global = true, default_value_t = DEFAULT_MIN_MATCHES, required = false)]
    pub motion_min_matches: usize,
}

pub async fn run(
    show_fps: bool,
    input_cfg: GstInputConfig,
    output_cfg: GstOutputConfig,
    flags: ProcessingChainFlags,
) -> Result<(), Box<dyn std::error::Error>> {
    gstreamer::init()?;

    let chain = build_processing_chain(flags)?;

    let (pipeline, input_handle, output_handle, mut stats_rx) = FramePipeline::new(chain);
    let output_writer = GstOutputWriter::try_new(output_cfg, pipeline.clone(), show_fps)?;
    let input_reader = GstInputReader::try_new(input_cfg, pipeline.clone())?;

    output_writer.start_output_thread(output_handle);
    input_reader.start_input_thread(input_handle);
    {
        let mut close_signal = pipeline.subscribe();

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            let stats = tokio::select! {
                _ctrl_c_result = &mut ctrl_c => {
                    tracing::warn!("Ctrl+C received, initiating graceful shutdown...");

                    pipeline.shutdown();
                    break;
                }
                _signal = close_signal.recv() => {
                    break;
                }
                stats = stats_rx.recv() => {
                    match stats {
                        Ok(stats) =>stats,
                        _ => continue,
                    }
                }
            };

            tracing::info!(
                frames_processed = stats.frames_processed,
                avg_fps = stats.avg_fps,
                dropped_frames = stats.dropped_frames,
                avg_processing_time_ms = format!("{:.6}", stats.avg_processing_time_ms),
                middleware_processing_times_ms = ?stats.middleware_processing_times.into_iter().map(| (key, val) |(key, format!("{:.2}", val.as_secs_f64() * 1000.0))).collect::<Vec<_>>(),
                "Pipeline statistics"
            );
        }
    }

    tracing::info!("Waiting for resource cleanup...");

    pipeline.closed().await;

    Ok(())
}

fn build_processing_chain(
    ProcessingChainFlags {
        scale,
        crop,
        smoothing_factor,
        display_stabilization,
        features_method,
        filter_type,
        display_features,
        display_motion_vectors,
        num_features,
        distance_to_border,
        fast_threshold,
        motion_min_matches,
    }: ProcessingChainFlags,
) -> Result<ProcessingChain, anyhow::Error> {
    let feature_frame_scale =
        Percent::try_new(f32::from(scale) / 100.0).ok_or(anyhow::anyhow!("invalid frame scale"))?;
    let stabilization_crop =
        Percent::try_new(f32::from(crop) / 100.0).ok_or(anyhow::anyhow!("invalid frame scale"))?;
    let stabilization_smoothing = Percent::try_new(f32::from(smoothing_factor) / 100.0)
        .ok_or(anyhow::anyhow!("invalid stabilization smoothing factor"))?;

    let mut chain = ProcessingChain::new()
        .add_middleware(middleware::MiddlewareDisplatch::Resize(
            ResizeMiddleware::new(
                feature_frame_scale,
                fast_image_resize::ResizeAlg::Convolution(filter_type.into()),
            ),
        ))
        .add_middleware(middleware::MiddlewareDisplatch::Grayscale(
            GrayscaleMiddleware::new(),
        ));

    let config = FeatureDetectionConfig {
        num_features,
        distance_to_border,
        fast_threshold,
    };

    chain = chain.add_middleware(match features_method {
        FeaturePointsMethod::Fast => {
            middleware::MiddlewareDisplatch::Fast(FastFeatureDetectionMiddleware::new(config))
        }
        FeaturePointsMethod::Orb => {
            middleware::MiddlewareDisplatch::Orb(OrbFeatureDetectionMiddleware::new(config))
        }
    });

    if display_features {
        chain = chain.add_middleware(middleware::MiddlewareDisplatch::FeaturePointOverlay(
            FeaturePointOverlayMiddleware::new(),
        ));
    }

    chain = chain
        .add_middleware(middleware::MiddlewareDisplatch::MotionMatches(
            MotionMatchesMiddleware::new(motion_min_matches),
        ))
        .add_middleware(middleware::MiddlewareDisplatch::MotionEstimation(
            MotionEstimationMiddleware::new(motion_min_matches),
        ));

    if display_motion_vectors {
        chain = chain.add_middleware(middleware::MiddlewareDisplatch::MotionMatchesOverlay(
            MotionMatchesOverlayMiddleware::new(),
        ));
    }

    chain = chain.add_middleware(middleware::MiddlewareDisplatch::Stabilization(
        StabilizationMiddleware::new(
            display_stabilization,
            stabilization_crop,
            stabilization_smoothing,
        ),
    ));

    Ok(chain)
}
