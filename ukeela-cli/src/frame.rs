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
#[derive(Clone, Default)]
pub struct Frame {
    pub id: u64,
    pub timestamp: Duration,
    pub width: usize,
    pub height: usize,
    pub format: VideoFormat,
    /// Actual pixel data - copied only when needed
    pub data: Arc<FrameData>,
    /// Metadata passed through pipeline
    pub metadata: FrameMetadata,
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
    pub fn channels(self) -> usize {
        match self {
            VideoFormat::BGR => 3,
            VideoFormat::BGRA => 4,
            VideoFormat::RGBA => image::Rgba::<u8>::CHANNEL_COUNT as usize,
        }
    }
}

#[derive(Default, Clone)]
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

#[derive(Debug, Clone, Copy, Default)]
pub struct SimilarityTransform {
    pub a: f64,
    pub b: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Frame {
    pub fn new(data: FrameData, width: usize, height: usize, format: VideoFormat) -> Self {
        static FRAME_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        Self {
            id: FRAME_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            timestamp: (UtcDateTime::now() - UtcDateTime::UNIX_EPOCH)
                .try_into()
                .expect("duration is always positive"),
            width,
            height,
            format,
            data: Arc::new(data),
            metadata: FrameMetadata::default(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
