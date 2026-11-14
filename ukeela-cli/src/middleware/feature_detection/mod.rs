use std::num::NonZero;

use crate::types::frame_scale::FrameScale;

pub mod fast;
pub mod orb;

#[derive(Debug, Clone, Copy)]
pub struct FeatureDetectionConfig {
    pub frame_scale: FrameScale,
    pub num_features: NonZero<usize>,
    pub distance_to_border: u32,
    pub fast_threshold: u8,
    pub debug_features: bool,
}

impl Default for FeatureDetectionConfig {
    fn default() -> Self {
        Self {
            frame_scale: FrameScale::try_new(0.25).expect("default is valid"),
            num_features: NonZero::new(200).expect("default is valid"),
            distance_to_border: 50,
            debug_features: false,
            fast_threshold: 30,
        }
    }
}
