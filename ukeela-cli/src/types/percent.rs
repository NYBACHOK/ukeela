#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Percent(f32);

impl Percent {
    pub fn try_new(scale: f32) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
            return None;
        }

        Some(Self(scale))
    }
}

impl From<Percent> for f32 {
    fn from(value: Percent) -> Self {
        value.0
    }
}

impl From<Percent> for f64 {
    fn from(value: Percent) -> Self {
        value.0.into()
    }
}
