# CHANGES

Addendum log (AGENTS.md Rule 0). Entries are appended, never edited.

## 2026-10-14 12:09 EDT — Camera view and capture plan

New planned work: camera-view (a live preview as a wgpu texture) and
camera-capture (one still photo) on libmorph's public surface. They replace
the `mobile-camera` passthrough. `camera-stream` (recording) is reserved
for later. libmobile's `camera` feature is renamed to `camera-capture`,
and it gains `camera-view`: the NDK camera plus a CAMERA permission
plugin. Desktop uses nokhwa; the web uses `getUserMedia`.
Plan: `docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md` (Draft),
built in the nine steps of section 9, one crate per commit.

## 2026-10-14 12:31 EDT — libmorph-build crate plan

New planned work: a `crates/libmorph-build` crate re-exporting
`libmobile::gradle` with libmorph's `links` key preset, so Android apps take
libmorph-build rather than libmobile as their build-dependency. The repository
root becomes a Cargo workspace. Plan:
`docs/design/20261014_02_LIBMORPH_BUILD_CRATE_PLAN.md` (Draft, approved),
built in the three steps of section 5.
