use std::time::Duration;

use ukeela_pipeline::FramePipeline;
use ukeela_processing_chain::{
    ProcessingChain, multi::MultiProcessingChain, single::SingleProcessingChain,
};
use ukeela_source_gstreamer::{
    GstInputConfig, GstOutputConfig, input::GstInputReader, output::GstOutputWriter,
};

use crate::{
    args::{ProcessingFlags, ProcessingType},
    middleware::build_processing_chain,
};

pub async fn run(
    show_fps: bool,
    input_cfg: GstInputConfig,
    output_cfg: GstOutputConfig,
    flags: ProcessingFlags,
    processing_type: ProcessingType,
) -> Result<(), Box<dyn std::error::Error>> {
    gstreamer::init()?;

    let middlewares = build_processing_chain(flags)?;
    let (pipeline, input_handle, output_handle) = match processing_type {
        ProcessingType::Single => {
            let mut chain = MultiProcessingChain::new();
            for middleware in middlewares {
                chain.add_middleware(middleware);
            }
            FramePipeline::from_chain(chain, Duration::from_secs(60))
        }
        ProcessingType::Multi => {
            let mut chain = SingleProcessingChain::new();
            for middleware in middlewares {
                chain.add_middleware(middleware);
            }
            FramePipeline::from_chain(chain, Duration::from_secs(60))
        }
    };

    let output_writer = GstOutputWriter::try_new(output_cfg, pipeline.clone(), show_fps)?;
    let input_reader = GstInputReader::try_new(input_cfg, pipeline.clone())?;

    output_writer.start_output_thread(output_handle);
    input_reader.start_input_thread(input_handle);

    let mut close_signal = pipeline.subscribe();
    tokio::select! {
        _ctrl_c_result = tokio::signal::ctrl_c() => {
            tracing::warn!("Ctrl+C received, initiating graceful shutdown...");
            pipeline.shutdown();
        }
        _signal = close_signal.recv() => {}
    }

    tracing::info!("Waiting for resource cleanup...");
    pipeline.closed().await;

    Ok(())
}
