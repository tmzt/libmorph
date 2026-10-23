---
title: Foreground service and system events
date: 2026-10-23
status: Draft
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
