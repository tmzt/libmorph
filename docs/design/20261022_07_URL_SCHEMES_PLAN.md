---
title: Opening the app from a URL scheme
date: 2026-10-22
status: In implementation
covers:
  - "Declaring custom URL schemes for a generated Android project
    (`gradle::Project::url_scheme`), through libmorph-build"
  - "`MorphApp::on_open_url`: a URL the app was launched or resumed with,
    delivered after `init`, in order, never lost on a cold start"
  - Never logging a URL's query or fragment, in libmobile and libmorph
  - macOS, left to a later plan (it needs an app bundle)
sources:
  - "handlr (owner-approved request, 2026-10-22, from the handlr session):
    `handlr pair` prints `handlr://<tailscale host>:7347/pair#handlr1<base58>`
    and a QR code of it. Scanning it with the camera app, or tapping it,
    should open the companion app with that URL. The fragment is a
    single-use pairing secret. handlr validates the URL itself
    (`libhandlr_proto::link::PairingPayload::decode`). It asked for a
    feature, declared schemes, Android cold-start and running delivery,
    macOS, one Rust API, no logging of the fragment, and no App Links."
  - "libmobile today: the core `bridge.IntentPlugin` already forwards the
    launch intent (from `onCreate`) and every later one (from `onNewIntent`)
    as `MobileEvent::Intent(IntentData { action, data, extras, is_new })`,
    with `data` set to `Intent.getDataString()`. The generated manifest
    already sets `launchMode=\"singleTop\"`. It has no way to declare a
    scheme, and `src/android/intent.rs` logs every intent in full
    (`libmobile: intent {intent:?}`), which would put the secret in
    logcat."
  - "libmorph today: the Android runner ignores `MobileEvent::Intent`, so
    apps never see intents."
  - AGENTS.md, "Platform Layering (libmobile and libdevice first)"
related:
  - handlr docs/design/20261022_06 (the pairing link)
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md
---

# Opening the app from a URL scheme

## 1. Goal

An app declares one or more custom URL schemes. A link with one of those
schemes, tapped or scanned with the camera, opens the app, and the app gets
the URL exactly once, after `init`, whether the link started the app or
arrived while it was running. URLs are untrusted input, which the app
validates. They are never logged past the path, because a query or fragment
may hold a secret.

This plan covers Android. macOS is a later plan (section 6).

## 2. libmobile

### 2.1 Declaring schemes

```rust
impl gradle::Project {
    /// Open the app for links with this scheme (`handlr` for `handlr://…`).
    /// Lowercase letters, digits, `+`, `-` and `.`, starting with a letter
    /// (RFC 3986); panics otherwise. May be called more than once.
    pub fn url_scheme(self, scheme: &str) -> Self;
}
```

For each scheme, the generated manifest's activity gets one more filter,
next to the launcher's:

```xml
<intent-filter>
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />
    <data android:scheme="handlr" />
</intent-filter>
```

- There is no `android:autoVerify` and no App Links. A custom scheme is
  enough here.
- `launchMode="singleTop"` is unchanged. A link that arrives while the app
  is in front goes to `onNewIntent`. One that arrives while it is in the
  background starts a new task, unless the activity is the task's root.
  On the S23 we check that this delivers through `onNewIntent` rather than
  creating a second activity. If it does not, the launch mode becomes
  `singleTask`, which is the open question in section 7.
- The filters are inserted by `Project::generate` into the rendered
  manifest, before `</activity>`, by a new pure function
  `codegen::url_scheme_filters(&[String]) -> String`, which is tested.
  `GradleProject`'s public fields do not change, so the change is additive.
- Hand-written Gradle projects (`libmobile.gradle` without `Project`) add
  their own filter. The README says how.

### 2.2 Redacting the log

`src/android/intent.rs` stops logging intents in full. It logs the action,
the data cut after its path (the scheme, host, port and path, and the
literal `<redacted>` when a query or fragment followed), the extra keys
without their values, and `is_new`. The cut is done by a new pure function,
`intent::redact_url(&str) -> String`. It is tested with
`handlr://h.ts.net:7347/pair#handlr1abc`, which gives
`handlr://h.ts.net:7347/pair<redacted>`, and with a query, an empty
string and text that is not a URL. `IntentData`'s `Debug` is unchanged.
Apps own what they log.

### 2.3 Example

The `wgpu` example declares `url_scheme("libmobile-wgpu")` and logs each
`MobileEvent::Intent`'s data, redacted. It is a libmobile example that
uses the feature.

## 3. libmorph

```rust
pub trait MorphApp {
    /// The app was opened with `url` (one of the schemes its build script
    /// declared), at launch or while running. Called after `init`, in the
    /// order the links arrived. The URL is untrusted input: validate it,
    /// and do not log it past its path.
    fn on_open_url(&mut self, ctx: &mut MorphContext, url: &str) {
        let _ = (ctx, url);
    }
}
```

- **Android runner:** on `MobileEvent::Intent` whose action is
  `android.intent.action.VIEW` and whose `data` is set, it queues the data.
  Intents without data, such as the launcher's, are not passed on. Each
  turn the runner takes the queue and calls `on_open_url` for each URL,
  once the app exists. A link that cold-starts the app arrives before
  `init`, because `IntentPlugin.onCreate` runs before the first window.
  It waits in the queue and is delivered right after `init` (and after
  `on_capture_ready` if that is also due), before the first `update`. It
  is never dropped. While the app is suspended, links are queued and
  delivered after `on_resume`.
- **Desktop and web:** `on_open_url` is never called by this plan.
  Scripted runs gain a command, `open <url>`, which calls it, so apps can
  test their handling headlessly. The script parser rejects a URL that
  contains whitespace.
- **libmorph-build:** `gradle_project(id)` returns libmobile's `Project`, so
  `.url_scheme("handlr")` works in the app's build script unchanged. The
  schemes are declared there, not in a `MorphApp` constant, because the
  build script runs before the app's crate compiles and cannot read its
  constants.
- **No Cargo feature.** handlr asked for one. But the bridge is already in
  every APK, nothing opens the app until a scheme is declared, and the hook
  has a default body. A feature would add nothing a caller can switch off.
  If the owner wants one anyway, it would be `deep-link` and would only
  gate the hook.
- libmorph never logs the URL.

## 4. Steps

1. **libmobile: schemes and redaction.** `Project::url_scheme`,
   `codegen::url_scheme_filters`, `intent::redact_url` and the new log line,
   with host tests: the filter XML, scheme validation, and the redaction
   cases.
   **Done when:** `cargo test --workspace` passes, and the generated
   project of the `wgpu` example (which now declares `libmobile-wgpu`)
   builds with `gradle assembleDebug` and has the filter in its merged
   manifest.
2. **libmobile on the S23.**
   **Done when:** on the owner's Galaxy S23 Ultra, with the `wgpu` example:
   - `adb shell am start -a android.intent.action.VIEW -d
     'libmobile-wgpu://example.ts.net:7347/pair#secret'` cold-starts it,
     and logcat shows `…/pair<redacted>`, never `secret`;
   - the same command while it is in front, and again while it is in the
     background, reaches the running activity through `onNewIntent`
     (`is_new: true`), with no second activity in `dumpsys activity
     activities`.
3. **libmobile released** as 0.2.2, on the owner's word.
4. **libmorph: `on_open_url`.** The hook, the Android runner's queue and
   ordering, and the `open <url>` script command. A scripted test checks
   that `open` URLs reach `on_open_url` in order, after `init`. A runner
   unit test checks that URLs queued before `init` or while suspended are
   delivered after `init` or `on_resume`, in order.
   **Done when:** tests and clippy pass on every host.
5. **libmorph on the S23.** A `urlopen` example (scheme `libmorph-urlopen`)
   shows the last URL's scheme and host as the clear colour's hue, and logs
   them, redacted.
   **Done when:** on the S23 Ultra:
   - a cold start with `am start … -d 'libmorph-urlopen://a/b#secret'`
     logs the URL once, after `init`;
   - one sent while running logs it once more;
   - the Samsung camera, scanning a QR code of a `libmorph-urlopen://`
     URL shown in a browser, opens the app with it;
   - `secret` never appears in logcat.
6. **Release** libmorph 0.3.2 and libmorph-build 0.2.2, taking libmobile
   0.2.2, on the owner's word. Then the handlr session gets the revision.
   handlr declares `url_scheme("handlr")` in its build script and passes
   the URL from `on_open_url` to `Input::Pair`.

## 5. Release

Additive in all three crates:
- libmobile: a new builder method and new pure functions. The log line
  changes, which is not API.
- libmorph: a defaulted hook and a new script command.
- libmorph-build: libmobile's new method appears through its re-export.

## 6. macOS (not in this plan)

A link can only open a registered app bundle. macOS reads
`CFBundleURLTypes` from an `.app`'s `Info.plist`, registered with Launch
Services, and delivers the URL as a `kAEGetURL` Apple Event, or through
`application:openURLs:`. A cargo-built binary is not a bundle, and
libmorph-build makes no macOS bundles today. So a desktop plan needs:
- a bundling step (an `.app` with `Info.plist`, from libmorph-build or a
  script);
- the Apple Event handler, in libdevice under the layering rule, using
  `objc2`;
- delivery into the winit loop.

Until then, the desktop companion can take the URL as a command-line
argument, which handlr's tty client may already do.

## 7. Open questions for the owner

1. **`singleTop` or `singleTask`.** `singleTop` is the current default for
   every generated project. If step 2 shows a background link creating a
   second activity, should generated projects switch to `singleTask`, or
   only projects that declare a scheme?
2. **A Cargo feature.** None is proposed (section 3). Should there be one
   anyway?
3. **macOS.** Should a follow-up plan cover the bundle and the Apple
   Event, or is a command-line argument enough for the desktop companion?

## Amendment — 2026-10-22 23:30 EDT: approved

The owner approved this plan on 2026-10-22, with libmobile's part written
out as its own plan, libmobile `docs/design/20261022_05_URL_SCHEMES_PLAN.md`.
That plan carries section 2 and steps 1 to 3 here, which are now built
from it. The status stays `Draft` until the first commit that implements
this plan's step 4.

Section 7 stands as follows:
1. `singleTop` or `singleTask`: answered after step 2, only if a
   background link creates a second activity.
2. A Cargo feature: none, as section 3 proposes.
3. macOS: not answered; this plan does not depend on it.

## Amendment — 2026-10-22 23:30 EDT: implemented in libmobile, wrapped here

On the owner's instruction. This replaces section 2 and steps 1 to 3, and
the approval amendment's sentence saying they "are now built from it".

The implementation of URL schemes moved to libmobile, under its plan
`docs/design/20261022_05_URL_SCHEMES_PLAN.md`: `Project::url_scheme`,
the intent filters, `redact_url` and the redacted intent log, its
example, its S23 check and its 0.2.2 release. Section 2 here is no longer
a specification; libmobile's plan is, and where they differ it wins.

libmorph only wraps it (steps 4 to 6):
- `MorphApp::on_open_url`, fed from libmobile's `MobileEvent::Intent`
  by the Android runner's queue, as section 3 describes;
- the `open <url>` script command;
- the `urlopen` example and its S23 check;
- the release taking libmobile 0.2.2.

libmorph-build adds no code: `gradle_project` already returns
libmobile's `Project`, so `url_scheme` reaches app build scripts
through it. Step 4 does not start until libmobile 0.2.2 is released.

## Amendment — 2026-10-23 18:04 EDT: implementation has started; the document is now immutable

Step 4 starts with this commit, against libmobile 0.2.2 (`eccea7c`, tag
`v0.2.2`), which completed steps 1 to 3 under libmobile's plan. The
document now changes only on the owner's explicit instruction.
