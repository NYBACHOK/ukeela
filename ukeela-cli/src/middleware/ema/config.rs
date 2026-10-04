/// Configuration for exponential moving average smoothing
#[derive(Debug, Clone)]
pub struct EmaConfig {
    pub alpha: f32,
    pub warmup_frames: usize,
    pub max_delta: f32,
}

impl Default for EmaConfig {
    fn default() -> Self {
        Self {
            alpha: 0.15,
            warmup_frames: 10,
            max_delta: 50.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MotionVector {
    pub dx: f32,
    pub dy: f32,
}

/// Batch motion data for SIMD processing
#[derive(Debug, Clone)]
pub struct MotionBatch {
    pub vectors: Vec<MotionVector>,
}

impl MotionBatch {
    pub fn new(capacity: usize) -> Self {
        Self {
            vectors: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, v: MotionVector) {
        self.vectors.push(v);
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }
}
