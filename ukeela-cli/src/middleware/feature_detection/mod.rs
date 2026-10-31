pub mod fast;
pub mod orb;

#[derive(Debug, Clone, Copy)]
pub struct FeatureDetectionConfig {
    pub frame_scale: f32,
    pub num_features: usize,
    pub distance_to_border: u32,
    pub fast_threshold: u8,
    pub debug_features: bool,
}

impl Default for FeatureDetectionConfig {
    fn default() -> Self {
        Self {
            frame_scale: 0.25,
            num_features: 200,
            distance_to_border: 50,
            debug_features: false,
            fast_threshold: 30,
        }
    }
}
