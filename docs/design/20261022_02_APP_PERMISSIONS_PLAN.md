---
title: Runtime permissions for apps
date: 2026-10-22
status: Draft
covers:
  - "`MorphContext::request_permissions`, `permission_granted` and
    `should_show_rationale`, the same on every host"
  - "A `MorphApp::on_permissions` hook for the answers"
  - "Routing answers by request id in the Android runner, replacing the
    capture code's `asking_capture` flag"
  - "The audiocapture example asking again after a denial"
sources:
  - "Owner (2026-10-22), after asking whether the camera and other
    permissions use the RECORD_AUDIO path, and whether `asking_capture` is
    audio only: \"write it up then implement in libmobile/libmorph. update
    example\"."
  - "libmobile `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` and its
    amendment \"request ids, one request at a time, and the rationale
    check\": `request_permissions(&AndroidApp, &[&str]) ->
    PermissionRequest`, `permission_granted`, `should_show_rationale`, and
    `MobileEvent::Permissions(PermissionsOutcome { request, permissions })`."
  - "libmorph today: `MorphContext` does not hold the `AndroidApp`. The
    Android runner requests `RECORD_AUDIO` itself for capture, marks it with
    `asking_capture: bool`, takes the next `Permissions` event as the answer,
    and ignores every other one."
related:
  - docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md (the capture request this
    replaces the flag of)
  - docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (on Hold; a live
    camera view would ask for CAMERA through this)
---

# Runtime permissions for apps

## 1. Goal

A libmorph app can ask for any runtime permission its manifest declares
(`CAMERA`, location, Bluetooth, notifications, ...), learn the answer, and
tell whether asking again would show a dialog. Code written for this
compiles and runs unchanged on hosts without runtime permissions.

libmorph's own request for capture uses the same route, and an answer is
never mistaken for another request's.

## 2. Types

In a new `src/permission.rs`, exported from the crate root. They are
libmorph's own, so the signatures are the same with and without `mobile`:

```rust
/// One `request_permissions` call, for matching its answer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PermissionRequest(u32);

/// The answer to one request, in request order.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct Permissions {
    pub request: PermissionRequest,
    /// Each permission's full name and whether it is granted.
    pub permissions: Vec<(String, bool)>,
}

impl Permissions {
    pub fn granted(&self, permission: &str) -> bool;
}
```

On Android, `From<libmobile::PermissionsOutcome> for Permissions` copies
the id and the list.

## 3. `MorphContext`

```rust
impl MorphContext {
    /// Ask the user for `permissions` (full names, declared in the
    /// manifest). The answer arrives in `MorphApp::on_permissions`. `None`
    /// where the host has no runtime permissions (desktop, web): carry on
    /// as if granted.
    pub fn request_permissions(&mut self, permissions: &[&str]) -> Option<PermissionRequest>;

    /// Whether `permission` is granted now. `true` off Android.
    pub fn permission_granted(&self, permission: &str) -> bool;

    /// After a denial: `true` if asking again shows a dialog, `false` if
    /// Android would deny it silently (send the user to Settings). `false`
    /// off Android.
    pub fn should_show_rationale(&self, permission: &str) -> bool;
}
```

On Android the context holds a clone of the `AndroidApp`
(`android: Option<AndroidApp>`, set before `init`) and calls libmobile.

## 4. The hook

```rust
pub trait MorphApp {
    /// The answer to a `ctx.request_permissions` call.
    fn on_permissions(&mut self, ctx: &mut MorphContext, answer: &Permissions) {
        let _ = (ctx, answer);
    }
}
```

It is called on the main thread, after the answer's `MobileEvent`, once per
request the app made. Answers to libmorph's own requests are not passed to
it.

## 5. Routing in the Android runner

The runner replaces `asking_capture: bool` with
`capture_request: Option<PermissionRequest>`, the id of the `RECORD_AUDIO`
request it made for capture. On `MobileEvent::Permissions(outcome)`:

1. If the id is `capture_request`, it clears it and opens capture if
   `RECORD_AUDIO` was granted, as today.
2. Otherwise it calls `on_permissions`.
3. In both cases, if capture was asked for in `audio_setup`, is still
   closed, and the answer grants `RECORD_AUDIO`, the runner opens capture
   and calls `on_capture_ready`. So an app that asks for `RECORD_AUDIO`
   again after a denial gets capture when the user allows it.

If the shell does not exist yet, the answer is dropped with a warning (it
cannot be: requests are made after `init`).

## 6. The example

`audiocapture` uses the new calls when capture is not ready:

- A tap on the dark red screen calls
  `ctx.request_permissions(&["android.permission.RECORD_AUDIO"])` and logs
  `permission: asked again (request <id>)`.
- `on_permissions` logs each answer. After a denial it also logs
  `ctx.should_show_rationale(..)`: when it is `false`, it logs
  `permission: denied for good; allow the microphone in Settings`.
- When the answer grants it, libmorph opens capture (section 5, item 3) and
  the spectrogram starts.

Taps after capture is ready still cycle the capture mode.

## 7. Steps

1. **libmobile.** Its amended plan's step 4 (ids, queue, rationale).
   **Done when:** that step's **Done when** holds.
2. **Types, context and hook.** Sections 2 to 4, with stubs off Android.
   Unit test of `Permissions::granted`.
   **Done when:** `cargo test -p libmorph` passes with and without default
   features, and clippy is clean on desktop and on Android with `mobile`.
3. **Routing.** Section 5.
   **Done when:** on the S23 the capture request still opens capture on
   "Allow", and an app request's answer reaches `on_permissions` and not
   the capture code.
4. **Example.** Section 6.
   **Done when:** on the S23, after a denial, a tap shows the dialog again.
   Allowing starts the spectrogram. Denying twice logs the Settings
   message, and a further tap is denied without a dialog.

## 8. Release

Additive (new types, methods and a defaulted hook), so it goes in the same
patch release as audio capture (0.2.2) if released together.
