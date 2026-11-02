use crate::{
    middleware::{
        chain::ProcessingChain,
        feature_detection::{
            FeatureDetectionConfig, fast::FastFeatureDetectionMiddleware,
            orb::OrbFeatureDetectionMiddleware,
        },
        grayscale::GrayscaleMiddleware,
        warp::WarpMiddleware,
    },
    pipeline::{
        FramePipeline,
        gst::{GstInputConfig, GstInputReader, GstOutputConfig, GstOutputWriter},
    },
};

pub mod frame;
pub mod middleware;
pub mod pipeline;

pub const DEFAULT_CHANELLS_SIZE: usize = 32;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display, clap::ValueEnum,
)]
#[display(rename_all = "lowercase")]
pub enum FeaturePointsMethod {
    Fast,
    Orb,
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
                current_width = stats.current_width,
                current_height = stats.current_height,
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
        crop_percent,
        features_method,
    }: ProcessingChainFlags,
) -> Result<ProcessingChain, anyhow::Error> {
    let mut chain = ProcessingChain::new();
    let feature_frame_scale = f32::from(crop_percent) / 100.0;

    chain = chain.add_middleware(middleware::MiddlewareDisplatch::Grayscale(
        GrayscaleMiddleware::new(),
    ));

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
        FeaturePointsMethod::Orb => middleware::MiddlewareDisplatch::Orb(
            OrbFeatureDetectionMiddleware::new(FeatureDetectionConfig {
                frame_scale: feature_frame_scale,
                num_features: 200,
                distance_to_border: 50,
                fast_threshold: 30,
                debug_features: false,
            })?,
        ),
    });
    chain = chain.add_middleware(middleware::MiddlewareDisplatch::Warp(
        WarpMiddleware::new(feature_frame_scale)?.with_crop_percent(crop_percent)?,
    ));
    chain = chain.add_middleware(middleware::MiddlewareDisplatch::FeaturePointOverlay(
        middleware::feature_point_overlay::FeaturePointOverlayMiddleware::new(feature_frame_scale)?,
    ));

    Ok(chain)
}
