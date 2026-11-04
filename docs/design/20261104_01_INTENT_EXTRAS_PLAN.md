---
title: An intent's own extras, delivered whole
date: 2026-11-04
status: In implementation
covers: >
  `MorphApp::on_intent`, which hands the app the whole intent libmobile
  already carries - its action, its data URI, every extra as a key and a
  string, and whether it arrived at an already-running instance. Today the
  Android runner takes `intent.data` and drops the rest, so a key and a
  payload sent with `am start --es <key> <value>` cannot reach an app at all.
  A script verb `intent <key>=<value>` delivers the same thing headlessly.
  `on_open_url` keeps its behaviour, so the addition is strictly additive and
  releases as 0.6.1.
sources: >
  `src/runner/android.rs:361` (the `MobileEvent::Intent` arm that keeps only
  `data`), `src/runner/shell.rs:48` and `:158` (`Lifecycle::open_url` and the
  flush after a resume, the pattern this follows), `src/app.rs:145`
  (`on_open_url`), `src/script.rs:132` and `:693` (the `open` verb),
  `src/runner/script.rs:122`, libmobile 0.3.5 `src/intent.rs`
  (`IntentData { action, data, extras, is_new }`, `extra(key)`, and its own
  docstring's `--es cmd refresh` example), libmobile
  `android/java/dev/highbay/libmobile/MobileNativeActivity.kt:23`
  (`launchMode="singleTop"`, so `onNewIntent` reaches a running app).
related: >
  `docs/design/20261022_07_URL_SCHEMES_PLAN.md`, whose `on_open_url` this
  sits beside and does not change. Asked for by sigilweaver, which sends a
  scene's Markdown to the phone as a base64 extra because libmorph's
  `args()` is empty on Android and a deep-link URL carries the payload
  through two shells; libmobile needs no change.
---

# An intent's own extras, delivered whole

## 1. What is wrong

libmobile carries the whole intent. `IntentData` is

```rust
pub struct IntentData {
    pub action: Option<String>,   // Intent.getAction()
    pub data: Option<String>,     // Intent.getDataString()
    pub extras: Vec<(String, String)>,   // every extra, toString()'d
    pub is_new: bool,             // false for the launch intent, true for onNewIntent
}
```

with an `extra(&self, key: &str) -> Option<&str>`, and its module docstring
states the idiom this plan exists to make reachable:

> `adb shell am start -n <pkg>/<activity> --es cmd refresh` arrives as an
> `IntentData` whose `extra("cmd")` is `Some("refresh")`.

libmorph drops all of it but one field. `src/runner/android.rs:361`:

```rust
MobileEvent::Intent(intent) if intent.action.as_deref() == Some(ACTION_VIEW) => {
    if let Some(url) = intent.data {
```

So an intent with no `data` is discarded entirely, an action other than
`VIEW` never arrives, and **no extra can reach an app through libmorph at
all**. `MorphApp` has `on_open_url` and no intent hook; libmobile's intent
module is push-only, with nothing to pull a current intent from, and
`run_android` owns the event loop - so an app on libmorph cannot work around
this from the outside.

**What asks for it.** sigilweaver sends a scene to the phone. `args()` is
documented "Empty on Android and the web", so the only channels are a deep
link and an intent. The scene is Markdown of about 10 KB: in a URL it is
base64 inside a path that `am start -d` parses twice, and the app then has to
pick the payload back out of its own link. As `--es scene_b64 <payload>` it is
a key and a value, which is what an intent is for.

## 2. The shape

One new type, one new hook, one new script verb. Nothing existing changes
its signature or its behaviour.

```rust
/// An intent the app was started with, or that arrived while it ran.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Intent {
    /// `Intent.getAction()`, e.g. `"android.intent.action.VIEW"`.
    pub action: Option<String>,
    /// `Intent.getDataString()`: the URI of a deep link.
    pub data: Option<String>,
    /// Every extra, stringified: `--ei n 3` arrives as `"3"`. The order is
    /// Android's bundle iteration order, which Android does not define.
    pub extras: Vec<(String, String)>,
    /// `false` for the intent the app was launched with, `true` for one
    /// delivered to an already-running instance.
    pub is_new: bool,
}

impl Intent {
    /// The value of extra `key`, if it is there.
    pub fn extra(&self, key: &str) -> Option<&str>;
}
```

libmorph keeps its own type rather than re-exporting libmobile's, because
the scripted runner builds one on the desktop where libmobile is not
compiled in. The fields are libmobile's, so the conversion is a move.

```rust
/// Every intent that reaches the app, whole: a deep link, a launcher tap
/// carrying extras, or `am start --es <key> <value>`.
fn on_intent(&mut self, ctx: &mut MorphContext, intent: &Intent) {
    let _ = (ctx, intent);
}
```

**Ordering, stated so it is not discovered.** A `VIEW` intent carrying a
`data` URI calls `on_intent` **first** and then `on_open_url`, because the
intent is the whole event and the URL is one field of it. An app may use
either or both; an app that uses only `on_open_url` sees exactly what it saw
before.

**Suspended delivery follows the links'.** `Lifecycle` already holds
`urls: Vec<String>` and releases them after `on_resume`; intents get
`intents: Vec<Intent>` beside it and the same treatment, so an intent that
arrives while the app is away is delivered once it is back, in order, after
`on_resume`.

**The extras are untrusted input**, like a URL: the doc comment says so, and
says to log a key rather than a value, since a value may be a secret.

## 3. The script verb

```text
intent <key>=<value> [<key>=<value> ...]
```

builds an `Intent` with no action, no data, `is_new: true` - a new intent
arriving at a running app, which is the case a script is for - and delivers
it through the same `Shell` path Android uses. `open <url>` already covers
the data leg, so this verb does not take one. A value may hold anything but
whitespace; the line takes no comment, as `open`'s does not.

## 4. Steps

**Step 1 - the type, the hook and the Android forwarding.** `src/intent.rs`
with `Intent` and `extra`, exported from `lib.rs`; `MorphApp::on_intent`
with an empty default body; `Lifecycle::intents` and `Shell::intent` beside
the URL pair; `runner/android.rs` converting every `MobileEvent::Intent` and
delivering it, then keeping the existing `VIEW`-with-`data` call to
`open_url`; the pre-shell buffer beside `self.urls` so an intent at launch is
not lost before `init`. Unit tests on `Lifecycle` for the suspended case, as
`shell.rs`'s existing ones do.

**Done when:** `cargo test -p libmorph` passes, a `Lifecycle` test shows an
intent arriving while suspended is delivered after the resume and in order,
and `cargo clippy -p libmorph --all-targets --target aarch64-linux-android`
is clean.

**Step 2 - the script verb and a scripted test.** `Command::Intent`,
`Action::Intent`, the parse with its usage string and its arity, and
`tests/intent.rs` modelled on `tests/open_url.rs`: a script sends
`intent k=v`, an app records what `on_intent` received, and the test asserts
the key, the value, `is_new`, and that `on_open_url` was not called.

**Done when:** `tests/intent.rs` passes, and a script with `suspend`,
`intent k=v`, `resume` delivers the intent after the resume.

**Step 3 - the documents and the release.** The `Core API` list in
`AGENTS.md`, a README decision row for this plan, a `CHANGES.md` addendum
recording what was built and any departure, `VERSIONS.md` for 0.6.1 listing
`Intent`, `Intent::extra`, `MorphApp::on_intent` and the `intent` verb as
additions, the version in `Cargo.toml`, and the tag.

**Done when:** `v0.6.1` is tagged on the commit that bumps the version, and
`VERSIONS.md` names every added item with no **Breaking** line.

## 5. What this does not do

No typed extras: everything arrives as a string, because that is what
libmobile's bridge sends and `--ei 3` and `--es 3` are indistinguishable by
the time they cross JNI. No sending of intents, no `startActivity`: this is
the receiving half only. No macOS equivalent - an `NSAppleEventManager` URL
is already `on_open_url`'s, and extras have no counterpart there. And
libmobile needs no change at all: every field this plan delivers is in
0.3.5 already.

## Amendment — 2026-11-04 00:26 EST: implementation has started

Steps 1 and 2 are built, so `status` is now `In implementation` and this
document is immutable: it changes only by amendment, and only on the owner's
explicit instruction.

One departure from section 2, found by the Android target's own clippy.
`Lifecycle::open_url` and `Shell::open_url` were guarded
`#[cfg(any(test, script_runner, target_os = "android"))]`, and with whole
intents forwarded nothing on Android calls either - a link now arrives
inside its intent and `deliver_intent` fans it out. Both are narrowed to the
script runner's cfg rather than left as dead code on the device. No public
item changes: `Shell` is private to the runner.
