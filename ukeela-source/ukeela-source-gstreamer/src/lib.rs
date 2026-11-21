use std::path::PathBuf;

pub mod input;
pub mod output;

#[derive(Debug, Clone)]
pub enum GstInputConfig {
    Camera { camera_device: String, fps: u32 },
    Libcamera { fps: u32, width: u32, height: u32 },
    File { path: PathBuf },
    Custom { pipeline: String },
}

#[derive(Debug, Clone)]
pub enum GstOutputConfig {
    FakeSink,
    Display {},
    File { fps: u32, path: PathBuf },
    Custom { pipeline: String },
}
