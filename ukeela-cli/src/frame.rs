use bytes::Bytes;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use time::UtcDateTime;

/// Shared frame buffer to minimize allocations
#[derive(Clone)]
pub struct Frame {
    pub id: u64,
    pub timestamp: Duration,
    pub width: u32,
    pub height: u32,
    pub format: VideoFormat,
    /// Actual pixel data - copied only when needed
    pub data: Arc<Bytes>,
    /// Metadata passed through pipeline
    pub metadata: Arc<RwLock<FrameMetadata>>,
}

#[derive(Clone, Debug)]
pub enum VideoFormat {
    I420,
    NV12,
    RGB,
    RGBA,
    BGR,
    BGRA,
}

#[derive(Default, Clone)]
pub struct FrameMetadata {
    /// Input order before pacing drops or reorders frames.
    pub original_order: Option<u64>,
    /// Sequential output order assigned after pacing and processing.
    pub order: Option<u64>,
    /// Monotonic time when the frame entered the pipeline.
    pub arrival_time: Option<Instant>,
    /// Per-feature displacements in feature-scaled pixel coordinates.
    pub motion_vectors: Option<Vec<(i16, i16)>>,
    /// Previous/current feature locations in feature-scaled pixel coordinates.
    pub motion_matches: Option<Vec<MotionMatch>>,
    pub feature_points: Option<Vec<(u32, u32)>>,
    pub estimated_transform: Option<[f32; 6]>,
    pub processing_time: HashMap<&'static str, Duration>, // Track time per middleware
}

/// A matched feature's coordinates in consecutive, feature-scaled frames.
pub type MotionMatch = ((f32, f32), (f32, f32));

impl Frame {
    pub fn new(data: Bytes, width: u32, height: u32, format: VideoFormat) -> Self {
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
            metadata: Arc::new(RwLock::new(FrameMetadata::default())),
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
