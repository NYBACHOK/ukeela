use std::num::NonZero;

pub mod fast;
pub mod orb;
pub mod overlay;

#[derive(Debug, Clone, Copy)]
pub struct FeatureDetectionConfig {
    pub num_features: NonZero<usize>,
    pub distance_to_border: u32,
    pub fast_threshold: u8,
}

impl Default for FeatureDetectionConfig {
    fn default() -> Self {
        Self {
            num_features: NonZero::new(100).expect("default is valid"),
            distance_to_border: 50,
            fast_threshold: 30,
        }
    }
}
