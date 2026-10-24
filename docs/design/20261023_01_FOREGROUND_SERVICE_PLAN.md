---
title: Foreground service and system events
date: 2026-10-23
status: In implementation
covers:
  - A generated Android foreground service (libmobile), declared from the
    app's build script with its service types, and controlled from Rust on
    any thread
  - "System events as a runtime-agnostic stream: foreground and
    background, the service's state, network changes, and Doze"
  - What the service does and does not guarantee about process lifetime on
    Android 14 to 16
  - The libmorph wrapper; desktop reports "unsupported"
sources:
  - "handlr (owner-instructed request, 2026-10-23, from the handlr
    session). The companion app (`dev.tmzt.handlr`, on libmorph 568cf95)
    keeps an encrypted link to the daemon on a Tokio current-thread runtime
    in its own thread (`crates/handlr-companion/src/net.rs`, link sockets
    with `SO_KEEPALIVE`). On the S23 the daemon sees the link close about 7
    seconds after the app is backgrounded, which fits Android 12+'s
    cached-app freezer. It needs to stay connected in the background, to
    speak notices such as \"Claude finished\" and to answer approvals. It
    asked for a generated service, a Rust API usable from any thread, async
    integration that does not tie libmorph to Tokio, and a statement of
    the lifetime guarantees."
  - "libmobile today: Kotlin plugins on `MobileNativeActivity`
    (`MobileActivityPlugin`), JNI calls through the Application context
    from `ndk-context` (`jni_util::with_application_class`, from the key
    store work), a generated manifest (`gradle::Project`), and a main loop
    that drains plugin mailboxes into `MobileEvent`s. The libmorph runner
    only calls `update` when it gets a frame, so nothing reaches the app
    from libmorph while it is in the background."
  - "Android: `Service.startForeground(int, Notification, int)` (API 29;
    the type argument is required from API 34); `foregroundServiceType`
    in the manifest and `FOREGROUND_SERVICE_<TYPE>` permissions (API 34);
    `ForegroundServiceStartNotAllowedException` for starts from the
    background (API 31); while-in-use types such as `microphone`, which
    must be started while the app is visible; `dataSync`'s limit of 6 hours
    in 24 and `Service.onTimeout(int, int)` (API 35); `POST_NOTIFICATIONS`
    (API 33); `ConnectivityManager.registerDefaultNetworkCallback` (API 24);
    `PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED`; the cached-app freezer
    (Android 12+), which freezes only cached processes."
  - AGENTS.md, "Platform Layering (libmobile and libdevice first)"
related:
  - docs/design/20261022_06_KEY_STORE_PLAN.md (the same shape: libmobile
    implements, libmorph wraps, calls work from any thread)
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md
---

# Foreground service and system events

## 1. Goal

An app can keep working while it is in the background:
- **Kept alive:** a foreground service, with its ongoing notification,
  stops Android from caching and freezing the process.
- **Told what happens:** the app's own threads, for example an async
  runtime, learn about foreground and background, the service, network
  changes and Doze. They learn through a stream that any executor can
  poll, and that does not depend on the UI loop running.

No app logic runs in Java, and libmobile and libmorph do not depend on an
async runtime. Android capabilities are built in libmobile, with libmorph
wrapping them (AGENTS.md, Platform Layering).

## 2. What the service guarantees, and what it does not

This section is the contract the app builds on. Items marked *checked* are
verified on the S23 in step 6.

- **Not frozen while it runs.** The cached-app freezer freezes only cached
  processes. A process with a started foreground service is not cached, so
  its threads keep running with the screen off, including the app's
  network thread and its async runtime. *Checked.*
- **The process can still die.** The service lowers the chance of being
  killed, but not to zero. Under memory pressure, or by the user's
  swipe-away or Force stop, the process can still die. The service does
  not restart the process or the link. If Android restarts the service
  (`START_STICKY` is not used), it does so without the app's state.
- **The CPU may sleep.** With the screen off, the CPU may sleep between
  events. Incoming packets on an open socket wake it, but the app's own
  timers (backoff, keepalives) can run late. An app that needs timers to
  fire on time while the screen is off holds a partial wake lock, through
  `ServiceOptions::wake_lock` (section 3.2). The cost is battery, so it is
  off by default.
- **Doze.** In Doze, network access is restricted, and how far a running
  foreground service is exempt varies by Android version and vendor. This
  plan does not promise network access in Doze. It reports Doze entry and
  exit (section 4), so the app can reconnect on exit. The S23 check
  (step 6) records what actually happens there with
  `dumpsys deviceidle force-idle`. *Checked, and recorded.*
- **Start rules.** The service must be started while the app is visible.
  Android refuses a start from the background
  (`ForegroundServiceStartNotAllowedException`), and the
  `microphone` type also needs `RECORD_AUDIO` granted. So an app starts it
  from a lifecycle hook while it is in front, for example on
  `on_resume`, and not later from its network thread. Updating and
  stopping work from anywhere, at any time.
- **Notification permission.** On Android 13+, if `POST_NOTIFICATIONS` is
  denied, the service still runs and is still foreground, but its
  notification does not appear in the drawer; it shows only in the
  system's running-apps list. This is reported as a warning
  (`ServiceEvent::NotificationHidden`), not an error.
- **Type limits.** `dataSync` is limited to 6 hours in any 24 on Android
  15+. At the limit Android calls `onTimeout`, the service must stop
  within seconds, and libmobile stops it and reports
  `ServiceEvent::TimedOut`. `specialUse` and `microphone` have no such
  limit. `specialUse` needs a declared reason
  (`PROPERTY_SPECIAL_USE_FGS_SUBTYPE`), which matters only for Play review.

For handlr, `specialUse` with the reason "persistent link to the user's
own server", started on resume, keeps the link alive without the 6-hour
limit. `microphone` is added only while listening for speech.

## 3. libmobile

### 3.1 The service and its manifest

Feature `foreground-service`. It stages
`android/java/dev/highbay/libmobile/service/MobileForegroundService.kt`, a
`Service`, and its plugin, `service.ForegroundServicePlugin`. The service:
- creates its notification channel, `libmobile.foreground`, of low
  importance;
- calls `startForeground(id, notification, types)`;
- updates the notification's text;
- handles `onTimeout`;
- holds a partial wake lock if asked;
- returns `START_NOT_STICKY`.

Its notification opens `MobileNativeActivity` when tapped, and uses the
app's launcher icon.

The app declares the types in its build script:

```rust
libmorph_build::gradle_project("dev.tmzt.handlr")
    .foreground_service(&[ServiceType::SpecialUse("persistent link to the user's own server"),
                         ServiceType::Microphone])
    .generate()?;
```

```rust
// libmobile::gradle
#[non_exhaustive]
pub enum ServiceType { DataSync, Microphone, SpecialUse(&'static str), RemoteMessaging }
impl Project { pub fn foreground_service(self, types: &[ServiceType]) -> Self; }
```

From the declared types, the generated manifest gets:
- `<service android:name="dev.highbay.libmobile.service.MobileForegroundService"
  android:exported="false" android:foregroundServiceType="specialUse|microphone">`;
- the subtype `<property>` for `specialUse`;
- the permissions `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_<TYPE>` for each
  type, `POST_NOTIFICATIONS`, and `WAKE_LOCK`.

`Microphone` also needs `RECORD_AUDIO`, which the app declares and asks
for itself, as it does now. Calling `foreground_service` without the
feature is a build-script error that names the feature.

### 3.2 Control from Rust

`src/android/service.rs`, re-exported as `libmobile::android::service`.
Every call is synchronous and works from any thread, through the
Application context:

```rust
pub struct ServiceOptions<'a> {
    pub types: &'a [ServiceType],   // a subset of the declared types
    pub title: &'a str,
    pub text: &'a str,
    pub wake_lock: bool,            // a partial wake lock while running
}
pub fn start(options: &ServiceOptions) -> Result<(), ServiceError>;
pub fn update(text: &str) -> Result<(), ServiceError>;   // the notification's text
pub fn stop() -> Result<(), ServiceError>;
pub fn is_running() -> bool;

#[non_exhaustive]
pub enum ServiceError {
    NotDeclared,              // the build script did not declare it, or not these types
    StartNotAllowed,          // started from the background
    MissingPermission(String),// e.g. RECORD_AUDIO for microphone
    NotRunning,               // update while stopped
    Platform(String),
}
```

`start` is answered synchronously as far as Android allows.
`startForegroundService` raises the background refusal immediately. A
failure inside the service's `startForeground`, such as a missing
while-in-use permission, arrives as `ServiceEvent::Failed` on the event
stream (section 4).

## 4. System events (libmobile, runtime-agnostic)

Feature `system-events`. It adds the core plugin `events.SystemEventsPlugin`
and `ACCESS_NETWORK_STATE`.

```rust
// libmobile::system_events (platform-free types, Android source)
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SystemEvent {
    Foreground(bool),                 // activity resumed / paused (app visible or not)
    Service(ServiceEvent),            // Started, Stopped, Failed(String), TimedOut, NotificationHidden
    Network(Option<NetworkInfo>),     // the default network: Some on available/changed, None on lost
    DeviceIdle(bool),                 // Doze entered / left
}
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NetworkInfo { pub validated: bool, pub metered: bool, pub transport: Transport /* Wifi, Cellular, Ethernet, Vpn, Other */ }

/// A subscription: every event after it was made, in order, never dropped
/// (unbounded; events are rare).
pub struct Events { /* Arc<Shared> */ }
pub fn subscribe() -> Events;
impl Events {
    pub fn try_next(&self) -> Option<SystemEvent>;
    pub fn next_blocking(&self) -> SystemEvent;            // for a plain thread
}
impl futures_core::Stream for Events { type Item = SystemEvent; } // any executor
/// The latest of each kind, for a subscriber that starts late.
pub fn current() -> SystemState;
```

- **How events are delivered.** They are pushed by the Kotlin callbacks
  straight into each subscriber's queue, on whatever Java thread runs the
  callback, and wake that subscriber's waker. They do not wait for the
  main loop or for frames. So a Tokio, smol or async-std task can
  `while let Some(e) = events.next().await`, and so can a thread with
  `next_blocking`, while the UI is paused.
- **Sources:**
  - activity lifecycle callbacks give `Foreground`;
  - the service gives `Service`;
  - `registerDefaultNetworkCallback` gives `Network`;
  - a receiver for `ACTION_DEVICE_IDLE_MODE_CHANGED`, reading
    `isDeviceIdleMode()`, gives `DeviceIdle`.

  They are registered on the Application context at the first
  `subscribe()`, and stay registered for the life of the process.
- **Dependencies:** `futures-core` 0.3 (the `Stream` trait only, with no
  executor) is the one new dependency. `async-channel` was considered;
  a small `Mutex<VecDeque>` with a stored `Waker` per subscriber is
  enough, at about 80 lines.
- **Tests:** the queue and waker logic is platform-free and tested on the
  host with a manual waker, and with `futures::executor::block_on` as a
  dev-dependency.

## 5. libmorph

- **Feature passthroughs:**
  `mobile-foreground-service = ["mobile", "libmobile/foreground-service"]`
  and `mobile-system-events = ["mobile", "libmobile/system-events"]`.
- **`libmorph::service`:** `start`, `update`, `stop`, `is_running`,
  `ServiceOptions`, `ServiceType` and `ServiceError`, the same on every
  host. Off Android, `start` returns `ServiceError::Unsupported`, a
  variant of libmorph's own, and the rest are no-ops. `MorphContext` is
  not needed, so the network thread calls them directly.
- **`libmorph::system_events`:** `subscribe() -> Events`, implementing
  `futures_core::Stream` with `try_next` and `next_blocking`, and
  `current()`. On Android it is libmobile's. On the desktop and the web it
  is libmorph's own, fed by the winit runner: `Foreground` from
  `resumed` and `suspended`, and nothing else. Network and idle events
  there are a later plan, in libdevice.
- **libmorph-build:** `gradle_project(..).foreground_service(..)` reaches
  apps through libmobile's `Project`. No code.

## 6. Steps

1. **libmobile: the service.** Feature `foreground-service`,
   `MobileForegroundService.kt` and its plugin, `Project::foreground_service`
   with the manifest entries (codegen tests), and `android::service`.
   **Done when:** `cargo test --workspace` passes, Android clippy is clean,
   and a generated project with each type builds with `gradle
   assembleDebug`.
2. **libmobile: system events.** Feature `system-events`, the plugin, the
   subscriber queue and `Stream`, and host tests.
   **Done when:** the same checks as step 1, plus the host tests for order,
   no loss and waking.
3. **libmobile example.** A `background` example: on its first resume it
   starts the service (`specialUse`, wake lock on) and spawns a plain
   thread that holds a TCP connection to a host given by an intent extra
   (`--es host <ip:port>`). Every 30 s it writes a line and logs the
   reply, logs each `SystemEvent`, and updates the notification's text
   with the count.
   **Done when:** on the owner's S23 Ultra:
   - with a listener on the Mac (`nc -lk 7999`), the example keeps the
     connection with the screen off and in the background for over
     10 minutes, and lines keep arriving every 30 s;
   - Home, then return, logs `Foreground(false)` and then `Foreground(true)`;
   - Wi-Fi off and on logs `Network(None)` and then `Network(Some(..))`;
   - `adb shell dumpsys deviceidle force-idle`, then `unforce`, logs
     `DeviceIdle(true)` and then `DeviceIdle(false)`, and records whether
     lines kept arriving in Doze;
   - with `POST_NOTIFICATIONS` revoked (`pm revoke`), the service still
     starts, logs `NotificationHidden`, and nothing crashes;
   - a start sent from the background (an `am broadcast` hook in the
     example) gives `StartNotAllowed`;
   - stopping removes the notification.
4. **libmobile released** as 0.2.3 (additive: new features, modules and a
   builder method), on the owner's word.
5. **libmorph wraps it.** The passthroughs, `libmorph::service`,
   `libmorph::system_events` (with the desktop's lifecycle-only stream),
   and a `background` example as in step 3. Its tests are the stream on the
   desktop and `Unsupported` off Android.
   **Done when:** tests and clippy pass on every host, and the example's
   10-minute background check passes on the S23.
6. **Release** libmorph 0.3.x and libmorph-build 0.2.x, after the URL
   schemes release, taking libmobile 0.2.3, on the owner's word. Then
   handlr gets the revision. In handlr, `on_resume` starts the service,
   and the net thread selects on `system_events::subscribe()`: it
   reconnects at once on `Network(Some)` and `DeviceIdle(false)`, and
   keeps its backoff otherwise.

## 7. Release

Additive in all three crates: new features, modules, types and a builder
method. `futures-core` is new, behind `system-events` in libmobile, and in
libmorph either always or behind its passthrough (open question 3).

## 8. Open questions for the owner

1. **Default service type for handlr.** This plan suggests `specialUse`,
   which has no time limit but needs a Play declaration. `dataSync` would
   give a 6-hour limit on Android 15+. Which should handlr use?
2. **Wake lock.** It is off by default and opt-in per start. Should
   handlr hold one while the service runs, so its backoff timers keep
   time with the screen off?
3. **libmorph's `futures-core`.** libmorph's own desktop stream also
   implements `Stream`. Should `futures-core` always be a libmorph
   dependency (it is tiny and has no executor), or only behind a feature?
4. **Desktop network events.** Should a later libdevice plan add network
   and sleep events on macOS (`NWPathMonitor`, power notifications), or is
   lifecycle-only enough for now?

## Amendment — 2026-10-23 01:27 EDT: futures support is an opt-in feature

The owner's answer to open question 3, relayed by the handlr session
(2026-10-23): "futures and compat need a positive feature". It settles
that question, and replaces the dependency and `Stream` lines of
sections 4, 5 and 7.

- **libmobile:** a feature `futures`, off by default, enables
  `system-events` and adds `futures-core` 0.3. Only with it do `Events`
  implement `futures_core::Stream`, along with any adapter helpers for async
  runtimes added later.
- **The default build:** `system-events` without `futures` has no
  `futures-core`, and `Events` offers only `try_next`, `next_blocking`
  and `current()`. The waker-based queue stays, since `Stream` is built
  on it, but registering a waker is internal until `futures` is on.
- **libmorph:** `futures = ["libmobile?/futures", ...]` forwards it. On the
  desktop, libmorph's own lifecycle stream implements `Stream` only under
  the same feature. Without it libmorph has no `futures-core` either.
- **Tests:** the host `Stream` tests run with `--features futures`, and
  the queue tests without it.
- **handlr** turns `futures` on in its companion app.

## Amendment — 2026-10-23 01:28 EDT: one feature per async runtime, none by default

The owner's correction, relayed by the handlr session (2026-10-23): "not
one generic `futures` feature. Use a specific feature for each runtime,
or a `-compat` variant". This replaces the amendment "futures support is
an opt-in feature" entirely, and the dependency and `Stream` lines of
sections 4, 5 and 7.

- **Default.** `system-events` on its own has no async dependency.
  `Events` offers `try_next`, `next_blocking` and `current()`, and
  nothing more.
- **One opt-in feature per runtime, named for it.** Native support uses
  the runtime's own primitive. A `-compat` variant (for example
  `tokio-compat`) is used only where an integration goes through a
  compatibility layer instead.
- **First to ship: `tokio`** (handlr's net thread is a Tokio
  current-thread runtime).
  - It adds `tokio` with `default-features = false, features = ["sync"]`,
    and `system_events::subscribe_tokio() ->
    tokio::sync::mpsc::UnboundedReceiver<SystemEvent>`.
  - The Java callbacks push into the sender directly. An unbounded
    send needs no runtime and works from any thread.
  - The app awaits `rx.recv()` in its `select!`.
- **Later, each when an app needs it,** each with its own amendment and
  following the same pattern:
  - `smol`, through `async-channel`'s `Receiver`, which is smol's own
    channel;
  - `-compat` variants such as `tokio-compat`.

  None is built now.
- **libmorph** carries the same feature names and forwards them
  (`tokio = ["libmobile?/tokio"]`, and its own desktop stream gains
  `subscribe_tokio` under it). Without a runtime feature, libmorph has no
  async dependency either.
- **`futures-core` is not a dependency** under any feature in this plan.
- **Tests:** the queue tests run with no features, and the `tokio`
  tests (`--features tokio`, with Tokio's `rt` and `macros` as
  dev-dependencies) check order and delivery from a non-runtime thread.

## Amendment — 2026-10-23 01:29 EDT: async features are named after their crates

The owner's refinement, relayed by the handlr session (2026-10-23): each
feature is named after the crate it brings in. This replaces the naming in
the amendment "one feature per async runtime, none by default" (its
`-compat` variants, such as `tokio-compat`). Everything else in that
amendment stands.

- Each async feature is the optional dependency's own name, so that
  `dep:` and the feature match: `tokio = ["dep:tokio", "system-events"]`.
- The names are `tokio` (built now), and later, each with its own
  amendment, `smol`, `async-compat` (an integration through the
  `async-compat` crate), and `futures-core` if a bare `Stream` impl is
  ever offered.
- libmobile and libmorph use the same names, off by default, with
  libmorph forwarding (`tokio = ["libmobile?/tokio", "dep:tokio"]`, the
  second for its own desktop stream).

## Amendment — 2026-10-23 01:40 EDT: review: visibility, battery exemption, stopping, Samsung, specialUse only

From the handlr session's review, made at the owner's direction
(2026-10-23), proposed for the owner's approval. It replaces the text
named in each item.

0. **Scope: `specialUse` only (owner).** The service work is aimed at
   handlr's `specialUse`, to keep its link alive.
   `ServiceType::Microphone` stays in the enum and the manifest code, but
   is not checked or recommended now. The handlr suggestion at the end of
   section 2 becomes: `specialUse`, started while visible. A background
   microphone is left to a later amendment. It would need `microphone` in
   the types at the visible start, since a running service's types cannot
   change.
1. **Samsung battery management** (adds to step 3). One UI's "Sleeping"
   and "Deep sleeping" apps, and its default battery setting, can stop
   background work even with a foreground service running. Step 3's
   10-minute and Doze checks are run twice, with the app's battery setting
   on "Unrestricted" and on the default, and both results are recorded,
   along with whether the app has to be on "Never sleeping apps".
2. **A battery-optimization exemption** (adds a module next to the
   service, in libmobile under `foreground-service`, wrapped by
   libmorph):

   ```rust
   pub mod battery {
       /// PowerManager.isIgnoringBatteryOptimizations(package).
       pub fn is_ignoring_optimizations() -> Result<bool, ServiceError>;
       /// ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS: the system's
       /// Allow/Deny dialog. Only while the app is visible; needs the
       /// build-script opt-in below, else NotDeclared.
       pub fn request_ignore_optimizations() -> Result<(), ServiceError>;
       /// ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS: the system list, for
       /// builds that may not ask directly.
       pub fn open_settings() -> Result<(), ServiceError>;
   }
   ```

   - `Project::request_ignore_battery_optimizations()`, off by default,
     declares `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`.
   - Play allows asking directly only for some kinds of app. A Play build
     uses `open_settings` instead, and the rustdoc says so.
   - The result of the dialog is read back with `is_ignoring_optimizations`
     on the next `Visible(true)`.
   - Step 3 compares the background and Doze results with and without
     the exemption. On Samsung, granting it shows as "Unrestricted", which
     step 3 confirms.
3. **Visibility, not focus** (replaces `Foreground(bool)` in section 4,
   and "on `on_resume`" in section 2's start rule). `SystemEvent` has
   `Visible(bool)` and `Focused(bool)`:
   - `Visible(bool)` comes from `onActivityStarted` and
     `onActivityStopped`. Android's start rule is about visibility, and
     an app in split screen or picture-in-picture is paused but visible.
   - `Focused(bool)` comes from `onActivityResumed` and
     `onActivityPaused`.

   Both come from `Application.registerActivityLifecycleCallbacks`,
   registered at the first `subscribe()`, so `MobileActivityPlugin` does
   not change. `current()` gives both. An app starts the service when it
   sees `Visible(true)` (or from `init` or `on_resume`, which happen while
   visible).
4. **No restart** (replaces section 2's "If Android restarts the service
   (`START_STICKY` is not used), …"). The service returns
   `START_NOT_STICKY`, so Android never restarts it after the process
   dies. Bringing it back is the app's job, the next time it is visible.
5. **Stopping:**
   - An app stops the service when it no longer needs to run in the
     background, for example after the user disconnects or signs out, or
     after unpairing.
   - **A "Disconnect" action on the notification.**
     `ServiceOptions::stop_action: Option<&str>` gives its label. Tapping
     it delivers `ServiceEvent::StopRequested`, and does not stop the
     service by itself, so the app can close its link and then call
     `stop()`.
   - **Swiping the app away from Recents.** The proposed default is that it
     stops the service: the generated `<service>` has
     `android:stopWithTask="true"`, and the app gets
     `ServiceEvent::Stopped` if its process is still alive. The reason is
     that a swipe is the user's way of closing the app.
     `Project::foreground_service_keeps_running()` opts out, giving
     `stopWithTask="false"` and `ServiceEvent::TaskRemoved` instead. This
     is open question 5.
   - Each case is checked in step 3: stop from the app, the action, and
     the swipe.
6. **Housekeeping.** The `futures-core` lines in section 4 (its
   dependency paragraph and the `impl futures_core::Stream` line), in
   section 5 (`implementing futures_core::Stream`) and in section 7 are
   superseded by the amendments "one feature per async runtime, none by
   default" and "async features are named after their crates". So is open
   question 3, which they answered.

**Open question 5:** should swiping the app away stop the service by
default (`stopWithTask="true"`, with an opt-out), or keep it running?

## Amendment — 2026-10-23 01:42 EDT: a generic notification

The owner's direction, relayed by the handlr session (2026-10-23): the
notification API is generic, with no app-specific concepts. This replaces
`ServiceOptions::stop_action` and `ServiceEvent::StopRequested` in the
amendment "review: visibility, battery exemption, stopping, Samsung,
specialUse only" (its item 5, second bullet). It also replaces
`ServiceOptions`' `title` and `text`, and `update(text)`, in section 3.2.

```rust
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NotificationContent {
    pub title: String,
    pub text: String,
    /// At most 3, Android's limit; more is NotificationError::TooManyActions.
    pub actions: Vec<NotificationAction>,
    /// Notification.VISIBILITY_PUBLIC / PRIVATE / SECRET on the lock screen.
    pub visibility: LockScreenVisibility,   // default Private
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NotificationAction {
    /// The app's own id, returned on a tap.
    pub id: String,
    pub label: String,
    /// Bring the activity forward on tap; nothing else.
    pub opens_app: bool,
}

pub struct ServiceOptions<'a> {
    pub types: &'a [ServiceType],
    pub content: &'a NotificationContent,
    pub wake_lock: bool,
}
pub fn update(content: &NotificationContent) -> Result<(), ServiceError>; // any thread
pub fn update_text(text: &str) -> Result<(), ServiceError>;               // a convenience
```

- **A tap on a button** is `ServiceEvent::Action(String)`, carrying the
  action's `id`. libmobile does nothing more with it: there is no built-in
  stop. The app decides, for example by calling `stop()`. Each action is a
  `PendingIntent` to the service, with the id as an extra; one with
  `opens_app` also brings `MobileNativeActivity` forward.
- **Tapping the body** opens the app, as before.
- **The wake lock** stays a start option. A toggle is the app's own
  action plus its own logic.
- **`ServiceError`** gains `TooManyActions` and `EmptyId` (an action with
  an empty id).
- **Step 3's example** shows two actions, `"ping"` (logged) and `"stop"`
  (the example calls `stop()` itself). It checks both, and the lock-screen
  visibility setting.
- handlr's text and buttons (its connection state, a disconnect button,
  approval counts) are its own content, built on these.

## Amendment — 2026-10-23 18:18 EDT: approved; implemented in libmobile, wrapped here

The owner approved this plan on 2026-10-23 ("approve plans in both
repos"). Its libmobile half is now libmobile's own plan,
`docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md` (approved, with its
amendment "no background microphone"). That plan replaces sections 2, 3
and 4 and steps 1 to 4 here, together with every amendment's libmobile
parts; where the two differ, libmobile's plan wins. It adds what the
owner asked for since this plan's last amendment: Termux-style
notification controls set at start (`Controls { ongoing, exit,
wake_lock_toggle, labels }`: a notification that comes back when swiped
away, an Exit button that stops the service and closes the app, and a
wake-lock toggle), `ServiceEvent::Exited`, `WakeLock(bool)` and
`TaskRemoved`, and `MobileEvent::Service`.

libmorph only wraps it (sections 5 and steps 5 and 6, as amended):
- the passthroughs `mobile-foreground-service`, `mobile-system-events`
  and `tokio`;
- `libmorph::service` (with `Controls`, `NotificationContent` and
  `battery`) and `libmorph::system_events`, the same on every host,
  `Unsupported` or lifecycle-only off Android;
- `MobileEvent::Service` passed to the app through a defaulted hook,
  named in step 5's wrapper with the owner's agreement if it is a new
  `MorphApp` hook;
- the `background` example and its S23 check;
- the release taking libmobile 0.2.3.

Section 8 and open question 5 stand as follows:
1. Service type: `specialUse` only (the amendment "review: …").
2. Wake lock: the user's, from the notification's toggle; it starts
   released.
3. `futures-core`: none (the async-feature amendments).
4. Desktop network and sleep events: not answered; lifecycle-only for now.
5. Swiping the app away stops the service by default, with an opt-out.

No microphone in the background: Android stops input when the app is
hidden. The status stays `Draft` until step 5's first commit, after
libmobile 0.2.3 is released.

## Amendment — 2026-10-24 12:27 EDT: implementation has started; the document is now immutable

Step 5 (libmorph wraps it) starts with this commit, against libmobile
0.2.3 (tag `v0.2.3`). The document now changes only on the owner's
explicit instruction.
