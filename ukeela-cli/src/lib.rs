use crate::{
    middleware::chain::ProcessingChain,
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
pub enum Backend {
    Auto,
    CPU,
    Vulkan,
}

#[derive(clap::ValueEnum, Clone, Debug, derive_more::Display)]
#[display(rename_all = "lowercase")]
pub enum StabilizationMode {
    None,
    SimpleSmoothing,
    #[display("l1-optimal")]
    L1Optimal,
    MeshWarp,
}

pub async fn run(
    mode: StabilizationMode,
    backend: Backend,
    channel_size: usize,
    input_cfg: GstInputConfig,
    output_cfg: GstOutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    gstreamer::init()?;

    let chain = build_processing_chain(mode, backend)?;

    let (_pipeline, input_handle, output_handle) = FramePipeline::new(chain, channel_size);
    let output_writer = GstOutputWriter::try_new(output_cfg)?;
    let input_reader = GstInputReader::try_new(input_cfg)?;

    output_writer.start_output_thread(output_handle);
    input_reader.start_input_thread(input_handle);

    tokio::signal::ctrl_c().await?;

    Ok(())
}

fn build_processing_chain(
    _mode: StabilizationMode,
    backend: Backend,
) -> Result<ProcessingChain, anyhow::Error> {
    let chain = ProcessingChain::new();

    // Add middleware based on config
    // if use_stabilization {
    //     // chain = chain.add_middleware(Arc::new(StabilizationMiddleware::new(config.mode)));
    // }

    if [Backend::Vulkan, Backend::Auto].contains(&backend) {
        // chain = chain.add_middleware(Arc::new(GpuWarperMiddleware::new()?));
    }

    Ok(chain)
}
