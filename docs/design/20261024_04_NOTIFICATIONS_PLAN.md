---
title: Alert notifications and notification taps
date: 2026-10-24
status: Draft
covers:
  - "`libmorph::notify::{post, cancel, cancel_all}`, `Channel`,
    `Importance`, `NotifyError`, under `mobile-notifications`"
  - "`MorphApp::on_notify(&mut self, ctx, &NotifyEvent)`, and
    `ServiceEvent::Tapped` through `on_service`"
  - "`NotificationContent::tag`, and updating the ongoing notification
    from any thread while suspended"
sources:
  - "handlr (request, 2026-10-24, with the owner's \"yes\") and the owner's
    \"updating the ongoing notification should be supported\"; see
    libmobile docs/design/20261024_04_NOTIFICATIONS_PLAN.md."
  - "libmorph 0.4.1: `libmorph::service` with libmorph's own
    `NotificationContent` and `ServiceEvent`, and `on_service`."
related:
  - libmobile docs/design/20261024_04_NOTIFICATIONS_PLAN.md
  - docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md
---

# Alert notifications and notification taps

## 1. What libmorph adds

- **`NotificationContent::tag: Option<String>`**, mapped to libmobile's.
- **`ServiceEvent::Tapped(Option<String>)`**, delivered to `on_service`.
- **`libmorph::notify`**: `post`, `cancel`, `cancel_all`, `Channel`,
  `Importance`, `NotifyError` (with `Unsupported`) and `NotifyEvent`,
  libmorph's own types over libmobile's, the same on every host. Off
  Android every call is `Unsupported` (desktop notifications are a later
  plan; macOS's need an app bundle).
- **`MorphApp::on_notify(&mut self, ctx, event: &NotifyEvent)`**, a new
  defaulted hook, called after `init`, suspended or not; a tap that
  cold-starts the app is delivered right after `init`.
- **Feature** `mobile-notifications = ["mobile", "libmobile/notifications"]`.
- **Updating:** `service::update` and `update_text` documented as working
  from any thread while suspended, as libmobile's plan section 3 makes
  them.
- Scripted runs: `notify-tap <id> [<tag>]` calls `on_notify` with a tap,
  so apps can test the response headlessly.

## 2. Steps

1. **The wrapper**, after libmobile's release, with tests: the mapping,
   `Unsupported` off Android, and `notify-tap` in a scripted run.
   **Done when:** tests and clippy pass on every host.
2. **Checked** with the `background` example as in libmobile's step 3,
   through libmorph.
   **Done when:** the alert, its tap (`on_notify`), the ongoing
   notification's tap (`on_service`, `Tapped`) and the updates while
   suspended all behave as there.
3. **Release** as the next libmorph version, on the owner's word.
   Additive.

## 3. Open question for the owner

1. **A new hook.** `on_notify` is new public API, needed because alert
   taps are not service events. Recommended: yes.

## Amendment — 2026-10-24 14:16 EDT: approved

The owner approved the notification plans in libmobile and libmorph on
2026-10-24 ("approve both"), with the recommended answers: the ongoing
notification's tap is a `ServiceEvent::Tapped` and alerts' taps a
`NotifyEvent`; `post` takes the channel each time, created on first use;
libmorph adds the `on_notify` hook. The status stays `Draft` until the
first commit that implements it.
