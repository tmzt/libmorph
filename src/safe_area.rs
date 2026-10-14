//! The region an app must keep its content inside, whatever host reported it.

/// Edge reservations in physical pixels, fractional so platform reports
/// (Android insets, scaled browser values) pass through without rounding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeArea {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl SafeArea {
    pub const NONE: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
}

/// An emulated device's insets, such as `spec.insets(InsetType::SafeDrawing)`.
/// Display dimensions are far below 2^24, so the conversion is exact.
impl From<libdevice::Insets> for SafeArea {
    fn from(insets: libdevice::Insets) -> Self {
        Self {
            top: insets.top as f32,
            right: insets.right as f32,
            bottom: insets.bottom as f32,
            left: insets.left as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SafeArea;
    use libdevice::{GALAXY_S24, InsetType, Insets};

    #[test]
    fn converts_each_side_by_name() {
        let insets = Insets {
            top: 1,
            right: 2,
            bottom: 3,
            left: 4,
        };
        assert_eq!(
            SafeArea::from(insets),
            SafeArea {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            }
        );
    }

    #[test]
    fn zero_insets_are_none() {
        assert_eq!(SafeArea::from(Insets::ZERO), SafeArea::NONE);
    }

    #[test]
    fn s24_safe_drawing_converts() {
        let safe = SafeArea::from(GALAXY_S24.insets(InsetType::SafeDrawing));
        assert_eq!((safe.top, safe.bottom), (110.0, 135.0));
    }
}
