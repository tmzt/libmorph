//! Scripted runs: the commands `media`, `media-camera`, `media-cancel`,
//! `media-deny` and `media-deny-forever` answer the waiting request, or the
//! next one. One request waits at a time; another is `Busy`, as on
//! Android.

use std::path::Path;

use super::{
    Delivery, MediaData, MediaError, MediaItem, MediaReply, MediaRequest, MediaRequestId,
    MediaResponse, MediaSource, Picker, mime_of,
};

/// A file a `media` command names, read by the runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScriptFile {
    /// The file's name, without its directory.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What a script command answers with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScriptAnswer {
    Files {
        files: Vec<ScriptFile>,
        source: MediaSource,
    },
    Cancel,
    Deny {
        settings_only: bool,
    },
}

#[derive(Debug, Default)]
pub(crate) struct Scripted {
    waiting: Option<(MediaRequestId, MediaRequest)>,
    /// An answer given before the request it answers.
    ready: Option<ScriptAnswer>,
}

impl Scripted {
    /// A request: answered now if an answer is ready (or another request
    /// waits: `Busy`), else it waits.
    pub(super) fn request(
        &mut self,
        id: MediaRequestId,
        request: &MediaRequest,
        cache: Option<&Path>,
    ) -> Option<MediaResponse> {
        if self.waiting.is_some() {
            return Some(respond(id, Err(MediaError::Busy)));
        }
        match self.ready.take() {
            Some(answer) => Some(respond(id, build(id, request, answer, cache))),
            None => {
                self.waiting = Some((id, request.clone()));
                None
            }
        }
    }

    /// A command's answer: for the waiting request, or kept for the next.
    pub(super) fn answer(
        &mut self,
        answer: ScriptAnswer,
        cache: Option<&Path>,
    ) -> Option<MediaResponse> {
        match self.waiting.take() {
            Some((id, request)) => Some(respond(id, build(id, &request, answer, cache))),
            None => {
                self.ready = Some(answer);
                None
            }
        }
    }
}

fn respond(id: MediaRequestId, result: Result<MediaReply, MediaError>) -> MediaResponse {
    match &result {
        // Counts and sizes, never names.
        Ok(reply) => log::info!(
            "libmorph: script: media {}: {} item(s), sizes {:?}, truncated {}",
            id.0,
            reply.items.len(),
            reply.items.iter().map(|i| i.size).collect::<Vec<_>>(),
            reply.truncated
        ),
        Err(error) => log::info!("libmorph: script: media {}: {error:?}", id.0),
    }
    MediaResponse {
        request: Some(id),
        result,
    }
}

fn build(
    id: MediaRequestId,
    request: &MediaRequest,
    answer: ScriptAnswer,
    cache: Option<&Path>,
) -> Result<MediaReply, MediaError> {
    let (files, source) = match answer {
        ScriptAnswer::Cancel => return Err(MediaError::Cancelled),
        ScriptAnswer::Deny { settings_only } => {
            return Err(MediaError::PermissionDenied { settings_only });
        }
        ScriptAnswer::Files { files, source } => (files, source),
    };
    if files.is_empty() {
        return Err(MediaError::Failed("no_item".into()));
    }
    let truncated = files.len() > request.items();
    let items = files
        .into_iter()
        .take(request.items())
        .enumerate()
        .map(|(n, file)| {
            let size = file.bytes.len() as u64;
            let mime = mime_of(&file.name);
            let data = if size > request.max_bytes {
                MediaData::TooLarge { size: Some(size) }
            } else {
                deliver(id, n, request.delivery, &file, cache)
            };
            MediaItem {
                source,
                uri: file.name.clone(),
                mime: mime.map(str::to_owned),
                display_name: Some(file.name),
                size: Some(size),
                exif_orientation: 0,
                data,
            }
        })
        .collect();
    Ok(MediaReply {
        items,
        picker: Picker::Chooser,
        camera_left_out: false,
        truncated,
    })
}

/// `Bytes`, or for `File` a copy in `<cache>/libmorph-media/<id>-<n>.<ext>`
/// (bytes when the run has no cache directory).
fn deliver(
    id: MediaRequestId,
    n: usize,
    delivery: Delivery,
    file: &ScriptFile,
    cache: Option<&Path>,
) -> MediaData {
    let (Delivery::File, Some(cache)) = (delivery, cache) else {
        return MediaData::Bytes(file.bytes.clone());
    };
    let ext = file
        .name
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let dir = cache.join("libmorph-media");
    let path = dir.join(format!("{}-{n}.{ext}", id.0));
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &file.bytes)) {
        Ok(()) => MediaData::File(path),
        Err(error) => MediaData::Unreadable(format!("{:?}", error.kind())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(n: usize) -> ScriptFile {
        ScriptFile {
            name: format!("p{n}.png"),
            bytes: vec![n as u8; 4],
        }
    }

    fn files(n: usize, source: MediaSource) -> ScriptAnswer {
        ScriptAnswer::Files {
            files: (0..n).map(png).collect(),
            source,
        }
    }

    #[test]
    fn waiting_request_is_answered_and_a_second_is_busy() {
        let mut s = Scripted::default();
        let request = MediaRequest::default();
        assert_eq!(s.request(MediaRequestId(1), &request, None), None);
        let busy = s.request(MediaRequestId(2), &request, None).unwrap();
        assert_eq!(busy.result, Err(MediaError::Busy));
        let answered = s.answer(files(1, MediaSource::Picked), None).unwrap();
        assert_eq!(answered.request, Some(MediaRequestId(1)));
        let reply = answered.result.unwrap();
        assert_eq!(reply.items.len(), 1);
        let item = &reply.items[0];
        assert_eq!(item.mime.as_deref(), Some("image/png"));
        assert_eq!(item.size, Some(4));
        assert_eq!(item.data, MediaData::Bytes(vec![0; 4]));
        assert_eq!(item.source, MediaSource::Picked);
    }

    #[test]
    fn ready_answer_serves_the_next_request_with_limits() {
        let mut s = Scripted::default();
        assert_eq!(s.answer(files(3, MediaSource::Camera), None), None);
        let request = MediaRequest {
            max_items: 2,
            max_bytes: 3,
            ..MediaRequest::default()
        };
        let reply = s
            .request(MediaRequestId(5), &request, None)
            .unwrap()
            .result
            .unwrap();
        assert!(reply.truncated);
        assert_eq!(reply.items.len(), 2);
        assert!(
            reply
                .items
                .iter()
                .all(|i| i.data == MediaData::TooLarge { size: Some(4) }
                    && i.source == MediaSource::Camera)
        );
    }

    #[test]
    fn cancel_and_deny() {
        let mut s = Scripted::default();
        let request = MediaRequest::default();
        s.request(MediaRequestId(1), &request, None);
        assert_eq!(
            s.answer(ScriptAnswer::Cancel, None).unwrap().result,
            Err(MediaError::Cancelled)
        );
        s.answer(
            ScriptAnswer::Deny {
                settings_only: true,
            },
            None,
        );
        assert_eq!(
            s.request(MediaRequestId(2), &request, None).unwrap().result,
            Err(MediaError::PermissionDenied {
                settings_only: true
            })
        );
    }

    #[test]
    fn file_delivery_copies_into_the_cache() {
        let dir = std::env::temp_dir().join(format!("libmorph-media-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Scripted::default();
        let request = MediaRequest {
            delivery: Delivery::File,
            ..MediaRequest::default()
        };
        s.request(MediaRequestId(9), &request, Some(&dir));
        let reply = s
            .answer(files(1, MediaSource::Picked), Some(&dir))
            .unwrap()
            .result
            .unwrap();
        let path = dir.join("libmorph-media").join("9-0.png");
        assert_eq!(reply.items[0].data, MediaData::File(path.clone()));
        assert_eq!(std::fs::read(&path).unwrap(), vec![0; 4]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
