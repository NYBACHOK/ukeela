mod input;
mod output;

use std::path::PathBuf;

pub use self::{input::*, output::*};

#[derive(Debug, Clone, clap::Subcommand)]
pub enum GstInputConfig {
    Camera {
        #[arg(required = true)]
        camera_device: String,
        #[arg(long, default_value_t = 1920, required = false)]
        width: u32,
        #[arg(long, default_value_t = 1080, required = false)]
        height: u32,
        #[arg(long, default_value_t = 30, required = false)]
        fps: u32,

        #[command(subcommand)]
        output: GstOutputConfig,
    },
    File {
        #[arg(required = true)]
        path: PathBuf,

        #[command(subcommand)]
        output: GstOutputConfig,
    },
    Custom {
        #[arg(required = true)]
        pipeline: String,

        #[command(subcommand)]
        output: GstOutputConfig,
    },
}

impl GstInputConfig {
    pub fn output(&self) -> GstOutputConfig {
        match self {
            GstInputConfig::Camera { output, .. } => output,
            GstInputConfig::File { output, .. } => output,
            GstInputConfig::Custom { output, .. } => output,
        }
        .clone()
    }
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum GstOutputConfig {
    Display {
        #[arg(short, long, default_value_t = 1920, required = false)]
        width: u32,
        #[arg(short, long, default_value_t = 1080, required = false)]
        height: u32,
    },
    File {
        #[arg(long, default_value_t = 1920, required = false)]
        width: u32,
        #[arg(long, default_value_t = 1080, required = false)]
        height: u32,
        #[arg(long, default_value_t = 30, required = false)]
        fps: u32,
        #[arg(required = true)]
        path: PathBuf,
    },
    Custom {
        #[arg(required = true)]
        pipeline: String,
    },
}
