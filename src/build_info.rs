//! The real device's vendor, make and model ([`MorphContext::build_info`]).
//!
//! [`MorphContext::build_info`]: crate::MorphContext::build_info

/// The device's vendor, make and model, and nothing that identifies the
/// unit or its owner: no serial, Android ID, IMEI or advertising ID.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuildInfo {
    /// Android: `Build.MANUFACTURER` ("samsung").
    pub vendor: String,
    /// Android: `Build.BRAND` ("samsung").
    pub make: String,
    /// Android: `Build.MODEL` ("SM-S918U").
    pub model: String,
}

#[cfg(feature = "mobile")]
impl From<libmobile::build_info::BuildInfo> for BuildInfo {
    fn from(info: libmobile::build_info::BuildInfo) -> Self {
        Self {
            vendor: info.manufacturer,
            make: info.brand,
            model: info.model,
        }
    }
}
