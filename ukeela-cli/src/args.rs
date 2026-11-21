use std::num::NonZero;

use ukeela_source_gstreamer::{GstInputConfig, GstOutputConfig};

pub(crate) const DEFAULT_MIN_MATCHES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, clap::ValueEnum)]
#[display(rename_all = "lowercase")]
pub enum ProcessingType {
    Single,
    Multi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, clap::ValueEnum)]
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
pub struct ProcessingFlags {
    /// Scale original image to specific percent of it's original size
    #[arg(long, global = true, default_value_t = 25, value_parser = clap::value_parser!(u8).range(1..101))]
    pub scale: u8,
    /// Crop this percentage from each edge for stabilization
    #[arg(long, global = true, default_value_t = 5, value_parser = clap::value_parser!(u8).range(1..51))]
    pub crop: u8,
    /// Weight of the latest motion estimate in smoothing (percent; higher is more responsive)
    #[arg(long, global = true, default_value_t = 50, value_parser = clap::value_parser!(u8).range(1..101))]
    pub smoothing_factor: u8,
    /// Draw the stabilization crop boundary without cropping
    #[arg(long, global = true, default_value_t = false)]
    pub display_stabilization: bool,
    #[arg(long, global = true, default_value_t = FeaturePointsMethod::Fast)]
    pub features_method: FeaturePointsMethod,
    #[arg(long, global = true, default_value_t = FilterType::Bilinear)]
    pub filter_type: FilterType,
    /// Show found features on image
    #[arg(long, global = true, default_value_t = false)]
    pub display_features: bool,
    /// Show optical-flow motion vectors on the image
    #[arg(long, global = true, default_value_t = false)]
    pub display_motion_vectors: bool,
    /// Use optical-flow-lk Shi-Tomasi detection and Lucas-Kanade tracking (ignores --features-method)
    #[arg(long, global = true, default_value_t = false)]
    pub optical_flow_lk: bool,
    #[arg(long, global = true, default_value_t = NonZero::new(100).expect("default is positive"))]
    pub num_features: NonZero<usize>,
    #[arg(long, global = true, default_value_t = 50)]
    pub distance_to_border: u32,
    #[arg(long, global = true, default_value_t = 30)]
    pub fast_threshold: u8,
    #[arg(long, global = true, default_value_t = DEFAULT_MIN_MATCHES)]
    pub motion_min_matches: usize,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum CliInputConfig {
    Camera {
        #[arg(required = true)]
        camera_device: String,
        #[arg(long, default_value_t = 30)]
        fps: u32,
        #[command(subcommand)]
        output: CliOutputConfig,
    },
    Libcamera {
        #[arg(long, default_value_t = 30)]
        fps: u32,
        #[arg(long, default_value_t = 1456)]
        width: u32,
        #[arg(long, default_value_t = 1088)]
        height: u32,
        #[command(subcommand)]
        output: CliOutputConfig,
    },
    File {
        #[arg(required = true)]
        path: std::path::PathBuf,
        #[command(subcommand)]
        output: CliOutputConfig,
    },
    Custom {
        #[arg(required = true)]
        pipeline: String,
        #[command(subcommand)]
        output: CliOutputConfig,
    },
}

impl CliInputConfig {
    #[must_use]
    pub fn output(&self) -> CliOutputConfig {
        match self {
            Self::Camera { output, .. }
            | Self::Libcamera { output, .. }
            | Self::File { output, .. }
            | Self::Custom { output, .. } => output.clone(),
        }
    }
}

impl From<CliInputConfig> for GstInputConfig {
    fn from(value: CliInputConfig) -> Self {
        match value {
            CliInputConfig::Camera {
                camera_device, fps, ..
            } => Self::Camera { camera_device, fps },
            CliInputConfig::Libcamera {
                fps, width, height, ..
            } => Self::Libcamera { fps, width, height },
            CliInputConfig::File { path, .. } => Self::File { path },
            CliInputConfig::Custom { pipeline, .. } => Self::Custom { pipeline },
        }
    }
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum CliOutputConfig {
    FakeSink,
    Display {},
    File {
        #[arg(long, default_value_t = 30)]
        fps: u32,
        #[arg(required = true)]
        path: std::path::PathBuf,
    },
    Custom {
        #[arg(required = true)]
        pipeline: String,
    },
}

impl From<CliOutputConfig> for GstOutputConfig {
    fn from(value: CliOutputConfig) -> Self {
        match value {
            CliOutputConfig::FakeSink => Self::FakeSink,
            CliOutputConfig::Display {} => Self::Display {},
            CliOutputConfig::File { fps, path } => Self::File { fps, path },
            CliOutputConfig::Custom { pipeline } => Self::Custom { pipeline },
        }
    }
}
