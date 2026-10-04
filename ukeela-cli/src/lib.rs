use crate::{middleware::chain::ProcessingChain, pipeline::FramePipeline};

pub mod frame;
pub mod middleware;
pub mod pipeline;

const POISONED_LOCK_MSG: &str = "poisoned lock";
pub const DEFAULT_CHANELLS_SIZE: usize = 32;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display, clap::ValueEnum,
)]
pub enum Backend {
    Auto,
    CPU,
    Vulkan,
}

#[derive(clap::ValueEnum, Clone, Debug, derive_more::Display)]
pub enum StabilizationMode {
    SimpleSmoothing,
    L1Optimal,
    MeshWarp,
}

pub async fn run(
    use_stabilization: bool,
    gpu_enabled: bool,
    backend: Backend,
    channel_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    gstreamer::init()?;

    let chain = build_processing_chain(use_stabilization, gpu_enabled, backend)?;

    let (pipeline, input_handle, output_handle) = FramePipeline::new(chain, channel_size);

    tokio::signal::ctrl_c().await?;

    Ok(())
}

fn build_processing_chain(
    use_stabilization: bool,
    gpu_enabled: bool,
    backend: Backend,
) -> Result<ProcessingChain, anyhow::Error> {
    let mut chain = ProcessingChain::new();

    // Add middleware based on config
    if use_stabilization {
        // chain = chain.add_middleware(Arc::new(StabilizationMiddleware::new(config.mode)));
    }

    if gpu_enabled && [Backend::Vulkan, Backend::Auto].contains(&backend) {
        // chain = chain.add_middleware(Arc::new(GpuWarperMiddleware::new()?));
    }

    Ok(chain)
}
