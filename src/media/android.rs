//! Android: libmobile's `media-request` (feature `mobile-media-request`),
//! with libmorph's types converted both ways and libmobile's request ids
//! mapped back to libmorph's.

use std::collections::HashMap;

use libmobile::android::AndroidApp;
use libmobile::media as mobile;

use super::{
    CameraEntry, CameraPermission, Delivery, MediaData, MediaError, MediaItem, MediaKind,
    MediaReply, MediaRequest, MediaRequestId, MediaResponse, MediaSource, Picker,
};

/// Forward `request` to libmobile; its answer is mapped back to `id`.
pub(super) fn request(
    ids: &mut HashMap<u32, MediaRequestId>,
    app: &AndroidApp,
    id: MediaRequestId,
    request: &MediaRequest,
) {
    let mobile_id = libmobile::android::media::request_media(app, &to_mobile(request));
    ids.insert(mobile_id.id(), id);
}

/// libmobile's answer: `request` is libmorph's id when libmorph made it in
/// this process, else `None`.
pub(super) fn response(
    ids: &mut HashMap<u32, MediaRequestId>,
    response: mobile::MediaResponse,
) -> MediaResponse {
    MediaResponse {
        request: response.request.and_then(|id| ids.remove(&id.id())),
        result: match response.result {
            Ok(reply) => Ok(from_reply(reply)),
            Err(error) => Err(from_error(error)),
        },
    }
}

fn to_mobile(request: &MediaRequest) -> mobile::MediaRequest {
    let mut out = mobile::MediaRequest::default();
    out.kind = match &request.kind {
        MediaKind::Images => mobile::MediaKind::Images,
        MediaKind::Videos => mobile::MediaKind::Videos,
        MediaKind::ImagesAndVideos => mobile::MediaKind::ImagesAndVideos,
        MediaKind::Any => mobile::MediaKind::Any,
        MediaKind::Mime(list) => mobile::MediaKind::Mime(list.clone()),
    };
    out.camera = match request.camera {
        CameraEntry::None => mobile::CameraEntry::None,
        CameraEntry::Photo => mobile::CameraEntry::Photo,
        CameraEntry::Video => mobile::CameraEntry::Video,
        CameraEntry::PhotoAndVideo => mobile::CameraEntry::PhotoAndVideo,
    };
    out.camera_permission = match request.camera_permission {
        CameraPermission::Ask => mobile::CameraPermission::Ask,
        CameraPermission::IfGranted => mobile::CameraPermission::IfGranted,
    };
    out.picker = match request.picker {
        Picker::Chooser => mobile::Picker::Chooser,
        Picker::PhotoPicker => mobile::Picker::PhotoPicker,
    };
    out.max_items = request.max_items;
    out.max_bytes = request.max_bytes;
    out.delivery = match request.delivery {
        Delivery::Bytes => mobile::Delivery::Bytes,
        Delivery::File => mobile::Delivery::File,
    };
    out.title = request.title.clone();
    out
}

fn from_reply(reply: mobile::MediaReply) -> MediaReply {
    MediaReply {
        items: reply.items.into_iter().map(from_item).collect(),
        picker: match reply.picker {
            mobile::Picker::PhotoPicker => Picker::PhotoPicker,
            _ => Picker::Chooser,
        },
        camera_left_out: reply.camera_left_out,
        truncated: reply.truncated,
    }
}

fn from_item(item: mobile::MediaItem) -> MediaItem {
    MediaItem {
        source: match item.source {
            mobile::MediaSource::Camera => MediaSource::Camera,
            mobile::MediaSource::Picked => MediaSource::Picked,
        },
        uri: item.uri,
        mime: item.mime,
        display_name: item.display_name,
        size: item.size,
        exif_orientation: item.exif_orientation,
        data: match item.data {
            mobile::MediaData::Bytes(bytes) => MediaData::Bytes(bytes),
            mobile::MediaData::File(path) => MediaData::File(path),
            mobile::MediaData::TooLarge { size } => MediaData::TooLarge { size },
            mobile::MediaData::Unreadable(why) => MediaData::Unreadable(why),
            other => MediaData::Unreadable(format!("{other:?}")),
        },
    }
}

fn from_error(error: mobile::MediaError) -> MediaError {
    match error {
        mobile::MediaError::Cancelled => MediaError::Cancelled,
        mobile::MediaError::Busy => MediaError::Busy,
        mobile::MediaError::NoApp => MediaError::NoApp,
        mobile::MediaError::NotVisible => MediaError::NotVisible,
        mobile::MediaError::PermissionDenied { settings_only } => {
            MediaError::PermissionDenied { settings_only }
        }
        mobile::MediaError::Failed(message) => MediaError::Failed(message),
        other => MediaError::Failed(other.to_string()),
    }
}
