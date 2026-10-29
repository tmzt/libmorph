//! **Asking the user for a photo, a video or a file** (plan
//! `20261028_02_MEDIA_REQUEST_PLAN.md`), with the same API on every host:
//! [`MorphContext::media_request`](crate::MorphContext::media_request)
//! returns at once, and the answer comes to
//! [`MorphApp::on_media`](crate::MorphApp::on_media).
//!
//! * **Android** (feature `mobile-media-request`): libmobile's
//!   `media-request`, Android's own chooser (the gallery, Files, Drive...)
//!   with an optional camera entry writing a full-size photo or video, or
//!   the Photo Picker. CAMERA, when the manifest declares it, is asked for
//!   by libmobile for the camera entry; its answer never reaches
//!   `on_permissions`, and a refusal is
//!   [`MediaError::PermissionDenied`]`{ settings_only }` (the app may then
//!   call [`MorphContext::open_app_settings`](crate::MorphContext::open_app_settings)).
//!   An answer that arrives before the app exists (the process was killed
//!   while the camera was up) is delivered right after `init`, with
//!   `request: None`.
//! * **The web** (feature `web`): a hidden `<input type="file">`. Call
//!   `media_request` while handling a touch or key: browsers open the file
//!   dialog only with a recent user gesture. Phones' browsers offer their
//!   camera beside the gallery by themselves; [`CameraEntry`] has no other
//!   effect there. Items always arrive as [`MediaData::Bytes`].
//! * **The desktop, and Android without the feature:**
//!   [`MediaError::Unsupported`] on the next frame.
//! * **Scripted runs** (desktop, `script`): the commands `media`,
//!   `media-camera`, `media-cancel`, `media-deny` and `media-deny-forever`
//!   answer (see [`script`](crate::script)).

#[cfg(all(target_os = "android", feature = "mobile-media-request"))]
mod android;
#[cfg(script_runner)]
mod scripted;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
mod web;

#[cfg(script_runner)]
pub(crate) use scripted::{ScriptAnswer, ScriptFile};

use std::fmt;
use std::path::PathBuf;

/// The default per-item cap, [`MediaRequest::max_bytes`]: 32 MiB.
pub const MEDIA_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// The ceiling on [`MediaRequest::max_items`].
pub const MEDIA_MAX_ITEMS: u16 = 100;

/// One `media_request` call, for matching its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaRequestId(pub(crate) u32);

impl MediaRequestId {
    /// The id as a number, for logs.
    #[must_use]
    pub fn id(self) -> u32 {
        self.0
    }
}

/// What the user may choose.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaKind {
    #[default]
    Images,
    Videos,
    ImagesAndVideos,
    /// Any openable file.
    Any,
    /// These MIME types (`"application/pdf"`, `"image/png"`, ...).
    Mime(Vec<String>),
}

/// The chooser's camera entries (Android).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraEntry {
    #[default]
    None,
    Photo,
    Video,
    PhotoAndVideo,
}

/// What to do about CAMERA for a camera entry, when the Android manifest
/// declares it and it is not granted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraPermission {
    /// Ask for it; a refusal fails the request
    /// ([`MediaError::PermissionDenied`]).
    #[default]
    Ask,
    /// Never ask; leave the camera entry out
    /// ([`MediaReply::camera_left_out`]).
    IfGranted,
}

/// Which system screen to show (Android).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Picker {
    /// The system chooser, with camera entries.
    #[default]
    Chooser,
    /// The Photo Picker when it can serve the request (no camera entry,
    /// images and/or videos, a device that has it); else the chooser.
    PhotoPicker,
}

/// How each item reaches the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Delivery {
    /// The bytes in memory.
    #[default]
    Bytes,
    /// A file under [`MorphContext::cache_dir`](crate::MorphContext::cache_dir),
    /// the app's to move or delete (Android and scripted runs; the web
    /// answers `Bytes`).
    File,
}

/// One request. Start from `MediaRequest::default()` and set the fields:
///
/// ```
/// use libmorph::media::{CameraEntry, MediaRequest};
/// let mut request = MediaRequest::default();
/// request.camera = CameraEntry::Photo;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaRequest {
    /// Default [`MediaKind::Images`].
    pub kind: MediaKind,
    /// Default [`CameraEntry::None`].
    pub camera: CameraEntry,
    /// Default [`CameraPermission::Ask`].
    pub camera_permission: CameraPermission,
    /// Default [`Picker::Chooser`].
    pub picker: Picker,
    /// 1 = single selection (the default). Clamped to
    /// `1..=MEDIA_MAX_ITEMS`.
    pub max_items: u16,
    /// Per item. Default [`MEDIA_MAX_BYTES`].
    pub max_bytes: u64,
    /// Default [`Delivery::Bytes`].
    pub delivery: Delivery,
    /// The chooser's title; `None` (the default) for the system's.
    pub title: Option<String>,
}

impl Default for MediaRequest {
    fn default() -> Self {
        Self {
            kind: MediaKind::Images,
            camera: CameraEntry::None,
            camera_permission: CameraPermission::Ask,
            picker: Picker::Chooser,
            max_items: 1,
            max_bytes: MEDIA_MAX_BYTES,
            delivery: Delivery::Bytes,
            title: None,
        }
    }
}

impl MediaRequest {
    /// `max_items` clamped to `1..=MEDIA_MAX_ITEMS`.
    #[cfg_attr(
        not(any(script_runner, all(target_arch = "wasm32", feature = "web"))),
        allow(dead_code)
    )]
    pub(crate) fn items(&self) -> usize {
        usize::from(self.max_items.clamp(1, MEDIA_MAX_ITEMS))
    }
}

/// Where an item came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSource {
    /// Chosen in the chooser, the picker or the browser's dialog.
    Picked,
    /// Taken through the chooser's camera entry (Android).
    Camera,
}

/// An item's content.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaData {
    Bytes(Vec<u8>),
    File(PathBuf),
    /// Over `max_bytes`; `size` when it is known.
    TooLarge {
        size: Option<u64>,
    },
    /// The item could not be read (for logs).
    Unreadable(String),
}

/// One chosen item.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaItem {
    pub source: MediaSource,
    /// Android: the `content://` URI as returned. The web and scripted
    /// runs: the file's name.
    pub uri: String,
    pub mime: Option<String>,
    pub display_name: Option<String>,
    pub size: Option<u64>,
    /// JPEG EXIF orientation 1..=8, 0 when absent or not read (the web and
    /// scripted runs). The pixels are not rotated.
    pub exif_orientation: u8,
    pub data: MediaData,
}

/// A request's items: never empty.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaReply {
    pub items: Vec<MediaItem>,
    /// Which screen was shown (`Chooser` off Android).
    pub picker: Picker,
    /// A camera entry was asked for but not offered.
    pub camera_left_out: bool,
    /// More than `max_items` came back; the rest were dropped.
    pub truncated: bool,
}

/// Why a request has no items.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaError {
    /// The user backed out.
    Cancelled,
    /// Another request is in flight.
    Busy,
    /// Nothing on the device handles the request.
    NoApp,
    /// The app was not visible, and Android does not let it start one.
    NotVisible,
    /// CAMERA refused for the camera entry. `settings_only`: asking again
    /// shows no dialog; send the user to
    /// [`MorphContext::open_app_settings`](crate::MorphContext::open_app_settings).
    PermissionDenied { settings_only: bool },
    /// No media chooser on this host or build.
    Unsupported,
    /// Anything else, for logs.
    Failed(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Busy => f.write_str("another media request is in flight"),
            Self::NoApp => f.write_str("no app handles the media request"),
            Self::NotVisible => f.write_str("the app is not visible"),
            Self::PermissionDenied { settings_only } => write!(
                f,
                "CAMERA denied{}",
                if *settings_only {
                    " (only Settings can grant it)"
                } else {
                    ""
                }
            ),
            Self::Unsupported => f.write_str("no media chooser on this host or build"),
            Self::Failed(message) => write!(f, "media request failed: {message}"),
        }
    }
}

impl std::error::Error for MediaError {}

/// The answer to one request, for
/// [`MorphApp::on_media`](crate::MorphApp::on_media).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaResponse {
    /// The request this answers; `None` when it was made before the Android
    /// activity or process was recreated.
    pub request: Option<MediaRequestId>,
    pub result: Result<MediaReply, MediaError>,
}

/// The MIME type of a file name's extension, for the hosts that have no
/// type of their own (scripted runs).
#[cfg_attr(not(any(test, script_runner)), allow(dead_code))]
pub(crate) fn mime_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "mp4" => "video/mp4",
        _ => return None,
    })
}

/// The media requests' state in the context: ids, the answers waiting for
/// the next frame, libmobile's ids on Android, the web's input element and
/// the stand-in of scripted runs.
#[derive(Debug, Default)]
pub(crate) struct MediaState {
    next_id: u32,
    /// Answered, delivered to `on_media` on the next frame.
    responses: Vec<MediaResponse>,
    /// libmobile's request ids to libmorph's.
    #[cfg(all(target_os = "android", feature = "mobile-media-request"))]
    mobile: std::collections::HashMap<u32, MediaRequestId>,
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    web: Option<web::WebMedia>,
    /// Set for scripted runs.
    #[cfg(script_runner)]
    script: Option<scripted::Scripted>,
}

impl MediaState {
    fn next(&mut self) -> MediaRequestId {
        self.next_id += 1;
        MediaRequestId(self.next_id)
    }

    #[cfg_attr(all(target_arch = "wasm32", feature = "web"), allow(dead_code))]
    fn push(&mut self, request: MediaRequestId, result: Result<MediaReply, MediaError>) {
        self.responses.push(MediaResponse {
            request: Some(request),
            result,
        });
    }

    /// The answers waiting, oldest first.
    pub(crate) fn take_responses(&mut self) -> Vec<MediaResponse> {
        #[cfg(all(target_arch = "wasm32", feature = "web"))]
        if let Some(web) = &self.web {
            self.responses.extend(web.take());
        }
        std::mem::take(&mut self.responses)
    }

    /// A scripted run: the stand-in answers.
    #[cfg(script_runner)]
    pub(crate) fn scripted(&mut self) {
        self.script = Some(scripted::Scripted::default());
    }

    /// `media`, `media-camera`, `media-cancel`, `media-deny`,
    /// `media-deny-forever`: answer the waiting request, or the next one.
    #[cfg(script_runner)]
    pub(crate) fn script_answer(&mut self, answer: ScriptAnswer, cache: Option<&std::path::Path>) {
        if let Some(script) = &mut self.script
            && let Some(response) = script.answer(answer, cache)
        {
            self.responses.push(response);
        }
    }

    /// libmobile's answer, in libmorph's terms.
    #[cfg(all(target_os = "android", feature = "mobile-media-request"))]
    pub(crate) fn android_response(
        &mut self,
        response: libmobile::media::MediaResponse,
    ) -> MediaResponse {
        android::response(&mut self.mobile, response)
    }
}

/// What a request needs from the context, besides the media state.
pub(crate) struct Host<'a> {
    /// Where `Delivery::File` copies go in scripted runs.
    #[cfg_attr(not(script_runner), allow(dead_code))]
    pub cache_dir: Option<&'a std::path::Path>,
    /// For libmobile's call; `None` before the activity is known.
    #[cfg(all(target_os = "android", feature = "mobile"))]
    #[cfg_attr(not(feature = "mobile-media-request"), allow(dead_code))]
    pub android: Option<&'a libmobile::android::AndroidApp>,
}

/// [`MorphContext::media_request`](crate::MorphContext::media_request).
pub(crate) fn request(
    state: &mut MediaState,
    host: Host<'_>,
    request: &MediaRequest,
) -> MediaRequestId {
    let id = state.next();
    #[cfg(script_runner)]
    if let Some(script) = &mut state.script {
        if let Some(response) = script.request(id, request, host.cache_dir) {
            state.responses.push(response);
        }
        return id;
    }
    #[cfg(all(target_os = "android", feature = "mobile-media-request"))]
    {
        match host.android {
            Some(app) => android::request(&mut state.mobile, app, id, request),
            None => state.push(
                id,
                Err(MediaError::Failed("the app is not running yet".into())),
            ),
        }
        id
    }
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    {
        let _ = host;
        let web = state.web.get_or_insert_with(web::WebMedia::default);
        if let Some(response) = web.request(id, request) {
            state.responses.push(response);
        }
        id
    }
    #[cfg(not(any(
        all(target_os = "android", feature = "mobile-media-request"),
        all(target_arch = "wasm32", feature = "web")
    )))]
    {
        let _ = (host, request);
        state.push(id, Err(MediaError::Unsupported));
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mirror_libmobile() {
        let r = MediaRequest::default();
        assert_eq!(r.kind, MediaKind::Images);
        assert_eq!(r.camera, CameraEntry::None);
        assert_eq!(r.camera_permission, CameraPermission::Ask);
        assert_eq!(r.picker, Picker::Chooser);
        assert_eq!((r.max_items, r.max_bytes), (1, 32 * 1024 * 1024));
        assert_eq!(r.delivery, Delivery::Bytes);
        assert_eq!(r.title, None);
        let mut r = r;
        r.max_items = 0;
        assert_eq!(r.items(), 1);
        r.max_items = 1000;
        assert_eq!(r.items(), 100);
    }

    #[test]
    fn mime_from_extension() {
        assert_eq!(mime_of("a.PNG"), Some("image/png"));
        assert_eq!(mime_of("a.jpg"), Some("image/jpeg"));
        assert_eq!(mime_of("a.b.jpeg"), Some("image/jpeg"));
        assert_eq!(mime_of("clip.mp4"), Some("video/mp4"));
        assert_eq!(mime_of("notes.txt"), None);
        assert_eq!(mime_of("noext"), None);
    }

    #[test]
    fn errors_display() {
        assert_eq!(
            MediaError::Unsupported.to_string(),
            "no media chooser on this host or build"
        );
        assert_eq!(
            MediaError::PermissionDenied {
                settings_only: true
            }
            .to_string(),
            "CAMERA denied (only Settings can grant it)"
        );
    }

    /// Without a backend (a desktop or Android build without the feature,
    /// not scripted), each call answers `Unsupported` with its id, once.
    #[cfg(not(any(
        all(target_os = "android", feature = "mobile-media-request"),
        target_arch = "wasm32"
    )))]
    #[test]
    fn unsupported_once_with_the_id() {
        let mut state = MediaState::default();
        let host = || Host {
            cache_dir: None,
            #[cfg(all(target_os = "android", feature = "mobile"))]
            android: None,
        };
        let a = request(&mut state, host(), &MediaRequest::default());
        let b = state.next();
        assert_ne!(a, b);
        assert_eq!(
            state.take_responses(),
            [MediaResponse {
                request: Some(a),
                result: Err(MediaError::Unsupported)
            }]
        );
        assert!(state.take_responses().is_empty());
    }
}
