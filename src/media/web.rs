//! The web: a hidden `<input type="file">`, created once and kept in the
//! document (plan 3.2). No `capture` attribute: with it, phones' browsers
//! go straight to the camera and hide the gallery. The browser opens the
//! dialog only with a recent user gesture; a call without one is ignored
//! by the browser, and the next call answers the old request `Cancelled`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{Event, FileList, HtmlInputElement};

use super::{
    CameraEntry, MediaData, MediaError, MediaItem, MediaKind, MediaReply, MediaRequest,
    MediaRequestId, MediaResponse, MediaSource, Picker,
};

/// The request the dialog is open for.
#[derive(Clone, Copy, Debug)]
struct Waiting {
    id: MediaRequestId,
    max_items: usize,
    max_bytes: u64,
    camera_left_out: bool,
}

/// Shared with the input's listeners.
#[derive(Default)]
struct Shared {
    waiting: Option<Waiting>,
    /// Answers for the next frame.
    inbox: Vec<MediaResponse>,
}

#[derive(Default)]
pub(crate) struct WebMedia {
    input: Option<HtmlInputElement>,
    shared: Rc<RefCell<Shared>>,
    /// Kept for as long as the input listens.
    listeners: Vec<Closure<dyn FnMut(Event)>>,
}

impl fmt::Debug for WebMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebMedia")
            .field("input", &self.input.is_some())
            .finish_non_exhaustive()
    }
}

/// `accept` for the kind: none for `Any`.
fn accept(kind: &MediaKind) -> String {
    match kind {
        MediaKind::Images => "image/*".into(),
        MediaKind::Videos => "video/*".into(),
        MediaKind::ImagesAndVideos => "image/*,video/*".into(),
        MediaKind::Mime(list) => list.join(","),
        _ => String::new(),
    }
}

/// A phone's browser offers its camera in the dialog by itself; a
/// heuristic only (`navigator.maxTouchPoints > 0`).
fn touch_browser() -> bool {
    web_sys::window().is_some_and(|w| w.navigator().max_touch_points() > 0)
}

impl WebMedia {
    /// Open the dialog for `request`; `Some` for an answer now (no document).
    pub(super) fn request(
        &mut self,
        id: MediaRequestId,
        request: &MediaRequest,
    ) -> Option<MediaResponse> {
        let input = match self.input() {
            Ok(input) => input,
            Err(error) => {
                return Some(MediaResponse {
                    request: Some(id),
                    result: Err(MediaError::Failed(error)),
                });
            }
        };
        {
            let mut shared = self.shared.borrow_mut();
            // Opened without a gesture, or never answered: it is over.
            if let Some(old) = shared.waiting.take() {
                shared.inbox.push(MediaResponse {
                    request: Some(old.id),
                    result: Err(MediaError::Cancelled),
                });
            }
            shared.waiting = Some(Waiting {
                id,
                max_items: request.items(),
                max_bytes: request.max_bytes,
                camera_left_out: request.camera != CameraEntry::None && !touch_browser(),
            });
        }
        let accept = accept(&request.kind);
        if accept.is_empty() {
            let _ = input.remove_attribute("accept");
        } else {
            input.set_accept(&accept);
        }
        input.set_multiple(request.items() > 1);
        // So choosing the same file again still fires `change`.
        input.set_value("");
        input.click();
        None
    }

    /// The answers that arrived, oldest first.
    pub(super) fn take(&self) -> Vec<MediaResponse> {
        std::mem::take(&mut self.shared.borrow_mut().inbox)
    }

    /// The input, created and wired up on first use.
    fn input(&mut self) -> Result<HtmlInputElement, String> {
        if let Some(input) = &self.input {
            return Ok(input.clone());
        }
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or("no document")?;
        let input = document
            .create_element("input")
            .map_err(|e| format!("{e:?}"))?
            .dyn_into::<HtmlInputElement>()
            .map_err(|_| "not an input element".to_owned())?;
        input.set_type("file");
        let _ = input.style().set_property("display", "none");
        document
            .body()
            .ok_or("no body")?
            .append_child(&input)
            .map_err(|e| format!("{e:?}"))?;

        let shared = Rc::clone(&self.shared);
        let source = input.clone();
        let change = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            let Some(waiting) = shared.borrow_mut().waiting.take() else {
                return;
            };
            let files = source.files();
            let shared = Rc::clone(&shared);
            wasm_bindgen_futures::spawn_local(async move {
                let result = read(files, waiting).await;
                shared.borrow_mut().inbox.push(MediaResponse {
                    request: Some(waiting.id),
                    result,
                });
            });
        });
        let shared = Rc::clone(&self.shared);
        let cancel = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            let mut shared = shared.borrow_mut();
            if let Some(waiting) = shared.waiting.take() {
                shared.inbox.push(MediaResponse {
                    request: Some(waiting.id),
                    result: Err(MediaError::Cancelled),
                });
            }
        });
        for (name, listener) in [("change", &change), ("cancel", &cancel)] {
            input
                .add_event_listener_with_callback(name, listener.as_ref().unchecked_ref())
                .map_err(|e| format!("{e:?}"))?;
        }
        self.listeners = vec![change, cancel];
        self.input = Some(input.clone());
        Ok(input)
    }
}

/// The chosen files, up to `max_items`, each under `max_bytes`.
async fn read(files: Option<FileList>, waiting: Waiting) -> Result<MediaReply, MediaError> {
    let Some(files) = files.filter(|f| f.length() > 0) else {
        return Err(MediaError::Failed("no_item".into()));
    };
    let count = files.length() as usize;
    let mut items = Vec::new();
    for index in 0..count.min(waiting.max_items) {
        let Some(file) = files.get(index as u32) else {
            continue;
        };
        let size = file.size() as u64;
        let name = file.name();
        let mime = Some(file.type_()).filter(|t| !t.is_empty());
        let data = if size > waiting.max_bytes {
            MediaData::TooLarge { size: Some(size) }
        } else {
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => MediaData::Bytes(web_sys::js_sys::Uint8Array::new(&buffer).to_vec()),
                Err(_) => MediaData::Unreadable("array_buffer".into()),
            }
        };
        items.push(MediaItem {
            source: MediaSource::Picked,
            uri: name.clone(),
            mime,
            display_name: Some(name),
            size: Some(size),
            exif_orientation: 0,
            data,
        });
    }
    log::info!(
        "libmorph: media {}: {} item(s), sizes {:?}",
        waiting.id.0,
        items.len(),
        items.iter().map(|i| i.size).collect::<Vec<_>>()
    );
    Ok(MediaReply {
        items,
        picker: Picker::Chooser,
        camera_left_out: waiting.camera_left_out,
        truncated: count > waiting.max_items,
    })
}
