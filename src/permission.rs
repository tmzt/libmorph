//! Runtime permissions: requests made with
//! [`MorphContext::request_permissions`](crate::MorphContext::request_permissions)
//! and their answers, delivered to
//! [`MorphApp::on_permissions`](crate::MorphApp::on_permissions). Only
//! Android has them; elsewhere there is nothing to ask for.

/// One `request_permissions` call, for matching its answer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PermissionRequest(pub(crate) u32);

/// The answer to one request, in request order.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct Permissions {
    /// The request this answers.
    pub request: PermissionRequest,
    /// Each permission's full name and whether it is granted.
    pub permissions: Vec<(String, bool)>,
}

impl Permissions {
    /// Whether `permission` was granted in this answer.
    #[must_use]
    pub fn granted(&self, permission: &str) -> bool {
        self.permissions
            .iter()
            .any(|(name, granted)| name == permission && *granted)
    }
}

#[cfg(all(target_os = "android", feature = "mobile"))]
impl From<libmobile::PermissionRequest> for PermissionRequest {
    fn from(request: libmobile::PermissionRequest) -> Self {
        Self(request.id())
    }
}

#[cfg(all(target_os = "android", feature = "mobile"))]
impl From<libmobile::PermissionsOutcome> for Permissions {
    fn from(outcome: libmobile::PermissionsOutcome) -> Self {
        Self {
            request: outcome.request.into(),
            permissions: outcome.permissions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PermissionRequest, Permissions};

    #[test]
    fn granted_names_only_granted_permissions() {
        let answer = Permissions {
            request: PermissionRequest(3),
            permissions: vec![("a".into(), true), ("b".into(), false)],
        };
        assert!(answer.granted("a"));
        assert!(!answer.granted("b"));
        assert!(!answer.granted("c"));
    }
}
