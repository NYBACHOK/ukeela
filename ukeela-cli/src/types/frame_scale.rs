#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct FrameScale(f32);

impl FrameScale {
    pub fn try_new(scale: f32) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
            return None;
        }

        Some(Self(scale))
    }
}

impl From<FrameScale> for f32 {
    fn from(value: FrameScale) -> Self {
        value.0
    }
}

impl From<FrameScale> for f64 {
    fn from(value: FrameScale) -> Self {
        value.0.into()
    }
}
