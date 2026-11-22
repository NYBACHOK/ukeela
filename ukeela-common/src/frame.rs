use image::Pixel;
use purecv::core::{KeyPoint, Matrix, Point};
use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use time::UtcDateTime;

pub type FrameData = image::ImageBuffer<image::Rgba<u8>, Vec<u8>>;

/// Shared frame buffer to minimize allocations
#[derive(Debug, Clone)]
pub struct Frame {
    pub id: u64,
    pub timestamp: Duration,
    pub width: usize,
    pub height: usize,
    pub format: VideoFormat,
    pub data: FrameDataBackend,
    /// Metadata passed through pipeline
    pub metadata: FrameMetadata,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            id: Default::default(),
            timestamp: Default::default(),
            width: Default::default(),
            height: Default::default(),
            format: Default::default(),
            data: FrameDataBackend::CPU(Default::default()),
            metadata: Default::default(),
        }
    }
}

/// Actual pixel data - copied only when needed
#[derive(Clone, Debug)]
pub enum FrameDataBackend {
    CPU(Arc<FrameData>),
    GPU(wgpu::Texture),
}

#[derive(Clone, Copy, Debug, Default)]
pub enum VideoFormat {
    BGR,
    BGRA,
    #[default]
    RGBA,
}

impl FromStr for VideoFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s.to_ascii_lowercase().as_str() {
            "bgr" => Self::BGR,
            "bgra" => Self::BGRA,
            "rgba" => Self::RGBA,
            _ => return Err(format!("unknow video format : {s}")),
        };

        Ok(res)
    }
}

impl VideoFormat {
    #[must_use]
    pub fn channels(self) -> usize {
        match self {
            VideoFormat::BGR => 3,
            VideoFormat::BGRA => 4,
            VideoFormat::RGBA => image::Rgba::<u8>::CHANNEL_COUNT as usize,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FrameMetadata {
    pub grayscale: Option<Arc<Matrix<u8>>>,
    /// Input order before pacing drops or reorders frames.
    pub original_order: Option<u64>,
    /// Sequential output order assigned after pacing and processing.
    pub order: Option<u64>,
    /// Monotonic time when the frame entered the pipeline.
    pub arrival_time: Option<Instant>,
    /// Previous/current feature locations in feature-scaled pixel coordinates.
    pub motion_matches: Vec<MotionMatch>,
    /// Detected feature locations in feature-scaled pixel coordinates.
    pub feature_points: Vec<KeyPoint>,
    pub similarity_transform: Option<SimilarityTransform>,
    pub processing_time: BTreeMap<&'static str, Duration>, // Track time per middleware
}

/// A matched feature's coordinates in consecutive, feature-scaled frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionMatch {
    pub previous: Point<f32>,
    pub current: Point<f32>,
}

/// Similarity transform mapping coordinates from the previous frame to the current frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimilarityTransform {
    pub a: f64,
    pub b: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Frame {
    #[inline]
    pub fn new_cpu(data: FrameData, width: usize, height: usize, format: VideoFormat) -> Self {
        Self::new(FrameDataBackend::CPU(Arc::new(data)), width, height, format)
    }

    #[inline]
    pub fn new_gpu(
        texture: wgpu::Texture,
        width: usize,
        height: usize,
        format: VideoFormat,
    ) -> Self {
        Self::new(FrameDataBackend::GPU(texture), width, height, format)
    }

    pub fn new(data: FrameDataBackend, width: usize, height: usize, format: VideoFormat) -> Self {
        static FRAME_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        Self {
            id: FRAME_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            timestamp: (UtcDateTime::now() - UtcDateTime::UNIX_EPOCH)
                .try_into()
                .expect("duration is always positive"),
            width,
            height,
            format,
            data,
            metadata: FrameMetadata::default(),
        }
    }

    pub fn frame_data_from_cpu(&self) -> &Arc<FrameData> {
        match &self.data {
            FrameDataBackend::CPU(image_buffer) => image_buffer,
            FrameDataBackend::GPU(_) => panic!("tried to get cpu data from gpu based frame"),
        }
    }

    pub fn frame_data_from_cpu_mut(&mut self) -> &mut Arc<FrameData> {
        match &mut self.data {
            FrameDataBackend::CPU(image_buffer) => image_buffer,
            FrameDataBackend::GPU(_) => panic!("tried to get cpu data from gpu based frame"),
        }
    }
}
