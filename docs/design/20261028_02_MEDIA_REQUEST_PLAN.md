---
title: Asking the user for a photo or video, on every host
date: 2026-10-28
status: In implementation
covers:
  - "Passthrough feature `mobile-media-request` for libmobile's
    `media-request`"
  - "`MorphContext::media_request(&MediaRequest) -> MediaRequestId` and
    `MorphApp::on_media(&mut self, ctx, &MediaResponse)`, the same API on
    every host"
  - "Android: libmobile's chooser with an optional camera entry; the web:
    a hidden `<input type=file>`; the desktop: `Unsupported` for now;
    scripted runs: commands that answer with files"
sources:
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"im thinking
    of media-request as open the media selector which has a camera
    option\"; then \"you can plan the media request in a new plan, we'll
    need it soon anyway\"."
  - "libmobile docs/design/20261028_02_MEDIA_REQUEST_PLAN.md (Draft): the
    `media-request` feature, `android::media::request_media`,
    `MobileEvent::Media(MediaResponse)`, the types of its section 5."
  - "docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (Hold), its
    amendment of 2026-10-28 13:35 EDT, item 3: a later `media-request` is
    a separate idea from the camera plans."
  - "libmorph today: `Cargo.toml:28` (`mobile-camera`, the passthrough
    pattern); `src/context.rs:223-266` (`request_permissions` and the
    rest, `cfg(all(target_os = \"android\", feature = \"mobile\"))` with a
    fallback), `:277-284` (`open_link`, with a scripted branch);
    `src/app.rs:121-127` (`on_permissions`, a hook with a default body);
    `src/runner/android.rs:176-188` (an answer that arrives before the app
    exists is dropped with a warning), `:330` (libmobile events mapped to
    hooks); `src/permission.rs:31-46` (libmorph's own types, `From`
    libmobile's under the Android cfg); `src/battery.rs:69,189-209`
    (`Unsupported` off Android); `Cargo.toml` `web-sys` features
    (`Document`, `Element`, `HtmlElement`, `Window` and a few more; no
    file input yet)."
  - "No native file dialog exists in libmorph or libdevice (libdevice has
    `args`, `clipboard`, `dirs`, `insets`, `key_store`, `link`,
    `secure_store`; no `NSOpenPanel`, no rfd)."
related:
  - libmobile docs/design/20261028_02_MEDIA_REQUEST_PLAN.md (the Android
    implementation; read it first)
  - docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md (Draft;
    `ctx.open_app_settings`, which this plan's `settings_only` answer
    points to)
  - docs/design/20261022_02_APP_PERMISSIONS_PLAN.md (the CAMERA answer
    is libmobile's and never reaches `on_permissions`)
---

# Asking the user for a photo or video, on every host

## 1. Goal

A libmorph app asks for an image (or video, or file) with one call, and
gets the chosen items' bytes in a hook, with the same code everywhere:

- **Android**: libmobile's `media-request` (its plan of the same date):
  the system chooser with gallery and file apps and, when asked, a camera
  entry; or the Photo Picker.
- **Web**: a hidden `<input type="file">`, which on phones' browsers
  offers the camera too.
- **Desktop**: `Unsupported` in this plan; a native dialog is question 1.
- **Scripted runs**: commands that answer with files from disk.

## 2. Feature

```toml
# Passthrough: `ctx.media_request` on Android, through libmobile's chooser.
mobile-media-request = ["mobile", "libmobile/media-request"]
```

It adds libmobile's provider and `<queries>` to the manifest (no
permission); libmorph-build forwards that through the existing Gradle
handoff unchanged. The web backend needs no feature beyond `web`, which
gains the web-sys features `HtmlInputElement`, `FileList`, `File`,
`Blob` and `Event`. `ctx.media_request` and `on_media` exist in every
build; without a backend the answer is `MediaError::Unsupported`.

`mobile-camera-intent` (the hand-off, after the Hold plan's rename) and
`mobile-camera` (the in-process camera) are independent of this
feature.

## 3. API

`src/media.rs`, always compiled. libmorph's own types, mirroring
libmobile's section 5 field for field, with `From` conversions under
`cfg(all(target_os = "android", feature = "mobile"))` as
`src/permission.rs` does:

```rust
pub use media::{MediaRequest, MediaRequestId, MediaKind, CameraEntry,
                CameraPermission, Picker, Delivery, MediaResponse,
                MediaReply, MediaItem, MediaData, MediaSource, MediaError};

impl MorphContext {
    /// Ask the user for media. Returns at once; the answer comes to
    /// `MorphApp::on_media` with this id. On the web, call it while
    /// handling a touch or key (3.2).
    pub fn media_request(&mut self, request: &MediaRequest) -> MediaRequestId;
}

// MorphApp, with a default body so existing apps compile unchanged:
fn on_media(&mut self, ctx: &mut MorphContext, response: &MediaResponse) {
    let _ = (ctx, response);
}
```

`MediaError` adds one variant to libmobile's: `Unsupported` ("no media
chooser on this host or build"). `MediaResponse::request` is
`Option<MediaRequestId>`, `None` for an answer to a request made before
the Android activity or process was recreated.

### 3.1 Android

- `media_request` calls `libmobile::android::media::request_media` and
  converts the id.
- `MobileEvent::Media` is routed to `on_media` in
  `src/runner/android.rs`, beside `Permissions` (`:330`).
- **Before the app exists.** After process death, libmobile's answer
  can arrive before the window, and so before the app object
  (`src/runner/android.rs:176-188` drops a permission answer in that case).
  Media answers are kept in a `Vec<MediaResponse>` on the runner and
  delivered right after `init`, in order. That is the only difference from
  the other hooks.
- **CAMERA.** libmobile asks for it for the camera entry (when the
  manifest declares it) and consumes the answer; it never reaches
  `on_permissions`. A refusal is `MediaError::PermissionDenied
  { settings_only }`; the app may call `ctx.open_app_settings()` (camera
  plan 3).
- `Delivery::File` paths are under `ctx.cache_dir()`.

### 3.2 The web

On `media_request`:

1. A hidden `<input type="file">` is created once and kept in the
   document. `accept` from the kind: `image/*`, `video/*`,
   `image/*,video/*`, none for `Any`, the MIME list joined with commas.
   `multiple` when `max_items > 1`.
2. **No `capture` attribute.** On Android and iOS browsers, `capture`
   sends the user straight to the camera and hides the gallery, which is
   `camera-intent`'s behaviour, not this one. Without it, a phone's
   browser already offers the camera beside the gallery for
   `accept="image/*"` (Chrome on Android lists Camera and Files; Safari
   on iOS offers Take Photo and Photo Library). So `CameraEntry` has no
   effect on the web; `camera_left_out` is `false` when a camera entry was
   asked for and the browser is a mobile one, else `true` (from
   `navigator.maxTouchPoints > 0`, a heuristic only).
3. `input.click()`. Browsers open the file dialog only with transient
   user activation: within a few seconds of a tap or key press (about 5 s
   in Chrome). libmorph's web input arrives from DOM listeners, so a call
   made from `on_touch` or `on_key`, or in the frame after, qualifies. A
   call without activation is ignored by the browser: no dialog and no
   event; the next call answers the old request `Cancelled` and opens
   anew.
4. `change`: for each `File` (up to `max_items`; `truncated` if more):
   `size` against `max_bytes` (`TooLarge { size }`), else
   `Blob::array_buffer()` through `wasm-bindgen-futures`, into
   `MediaData::Bytes`. `uri` is the file's name (there is no URI),
   `display_name` the name, `mime` its `type` when not empty, `source`
   always `Picked` (the browser does not say whether it came from the
   camera), `exif_orientation` 0 (not read).
5. `cancel` (Chrome 113+, Firefox 91+, Safari 16.4+): `Cancelled`.
6. `Delivery::File` is answered as `Bytes` (there is no file system);
   `Picker` is ignored and reported as `Chooser`.

### 3.3 The desktop

`Unsupported`, answered on the next frame, as `battery` does
(`src/battery.rs:189-209`). Question 1 asks about a dialog.

### 3.4 Scripted runs

Under `script` (desktop, headless):

- `media <path> [<path> ...]`: answers the pending request (or the next
  one) with those files, paths relative to the script, MIME type from the
  extension (`png`, `jpg`/`jpeg`, `mp4`, else none), `source: Picked`,
  `max_bytes` applied.
- `media-camera <path>`: the same with `source: Camera`.
- `media-cancel`, `media-deny`, `media-deny-forever`: `Cancelled`,
  `PermissionDenied { settings_only: false | true }`.

The script log prints counts and sizes, not names.

## 4. Steps

Each step one commit to libmorph, passing `cargo fmt -p libmorph --
--check`, `cargo clippy -p libmorph --all-targets` (desktop, wasm32 with
`web`, and `aarch64-linux-android` with `mobile` and
`mobile-media-request`) and `cargo test -p libmorph`. libmobile's steps
1-5 are released before step 2 here.

1. **Types, the call, the hook, the stub** (3, 3.3): every host answers
   `Unsupported`. Unit test: one `Unsupported` answer with the call's id
   on the next frame.
   **Done when:** the checks pass on all three targets without the
   feature.
2. **Android** (2, 3.1): the passthrough, the `From` conversions, the
   call forwarded, `MobileEvent::Media` routed, answers held until
   `init`.
   **Done when:** the checks pass with `mobile-media-request`, and a
   generated manifest has the provider only with it.
3. **Web** (3.2): the input element, `accept` and `multiple`, `change`
   and `cancel`, the size cap.
   **Done when:** the checks pass, and in Chrome on a desktop and on the
   owner's S23 (asked first) a tap in the `touch` example built with a
   media button opens the dialog and the chosen image's size is logged.
4. **Scripted runs** (3.4): the five commands. Tests: a script that
   requests, answers with a PNG and checks the app saw its bytes; a
   cancel; a denial.
   **Done when:** those tests pass.
5. **Example** (its own commit): `examples/android/mediarequest`, a
   button that asks for an image with a photo camera entry and draws the
   result, and a Settings button after `settings_only`.
   **Done when:** on the owner's S23 (asked first) libmobile step 6's
   chooser, camera and cancel checks pass through libmorph, and an APK
   without `mobile-media-request` has no provider.
6. **AGENTS.md and README** (separate commits): the passthrough in
   "Cargo Features & Passthrough", the call in "Core API", and this
   plan's row updated.
   **Done when:** both list the feature and the call.

## 5. Release

Additive: one feature, a new module, a `MorphContext` call and a
`MorphApp` hook with a default body. A libmorph patch release, taking
libmobile's release with `media-request`. If that libmobile release is
0.3.0 (the camera rename), libmorph's release is a minor one under Rule
5.5 for that reason, marked **Breaking** for the dependency, not for this
feature.

## 6. Open questions

1. **A desktop dialog.** A libdevice `open-panel` feature (`NSOpenPanel`
   on macOS through the objc2 crates it already uses), rfd (every
   desktop, but a new dependency tree and main-thread rules on macOS), or
   `Unsupported` until a desktop app needs it?
2. **The web's camera hint.** Leave `capture` off always (3.2), or set
   it when the request asks for a camera entry and nothing else matters
   (which then hides the gallery)?
3. **Holding early answers.** Deliver answers that arrive before the app
   exists right after `init` (3.1), or drop them as permission answers are
   dropped today?
4. **Script command names.** `media`, `media-camera`, `media-cancel`,
   `media-deny`, `media-deny-forever`?

## Amendment — 2026-10-29 01:53 EDT: approved

The owner approved the plan on 2026-10-29 ("approve media requests"). It
is approved as written; its open questions keep the smallest choices, each
changeable by a later amendment: (1) the desktop answers `Unsupported`
until a desktop app needs a dialog; (2) the web's `capture` stays off; (3)
answers that arrive before the app exists are delivered right after
`init` (3.1); (4) the script commands are `media`, `media-camera`,
`media-cancel`, `media-deny` and `media-deny-forever`. It builds on
libmobile's media request (its plan 20261028_02, approved the same day).
Its version is the next free patch at its commit. The status stays `Draft`
until the first implementing commit (Rule 1.5).

## Amendment — 2026-10-29 02:22 EDT: implementation has started; the document is now immutable

Steps 1 to 4 are the first implementing commit, as libmorph 0.5.2 on
libmobile 0.3.4 (its media request plan, built the same day), per the
approval amendment of 2026-10-29 01:53 EDT. Step 3's browser checks and
step 5 (the Android example and its S23 checks) are not done; step 6 is in
its own commits. The departures are recorded in `CHANGES.md`.
