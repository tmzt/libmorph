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

/// An Android inset report (`MobileEvent::Insets(insets)` → `insets.safe_area`).
/// libmobile orders its fields left-first; they are matched by name.
#[cfg(feature = "mobile")]
impl From<libmobile::SafeArea> for SafeArea {
    fn from(safe_area: libmobile::SafeArea) -> Self {
        Self {
            top: safe_area.top,
            right: safe_area.right,
            bottom: safe_area.bottom,
            left: safe_area.left,
        }
    }
}

impl SafeArea {
    /// Scale each edge, e.g. from emulated-device pixels to a window that
    /// came out a different size.
    #[must_use]
    pub fn scaled(self, x: f32, y: f32) -> Self {
        Self {
            top: self.top * y,
            right: self.right * x,
            bottom: self.bottom * y,
            left: self.left * x,
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

    #[test]
    fn scaled_uses_each_axis() {
        let safe = SafeArea {
            top: 10.0,
            right: 20.0,
            bottom: 30.0,
            left: 40.0,
        };
        assert_eq!(
            safe.scaled(0.5, 2.0),
            SafeArea {
                top: 20.0,
                right: 10.0,
                bottom: 60.0,
                left: 20.0,
            }
        );
    }

    #[cfg(feature = "mobile")]
    #[test]
    fn libmobile_converts_by_name() {
        let mobile = libmobile::SafeArea::new(4.0, 1.0, 2.0, 3.0);
        assert_eq!(
            SafeArea::from(mobile),
            SafeArea {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            }
        );
    }
}
