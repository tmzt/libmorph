# VERSIONS

Each change of a crate version in this workspace and the public API changes
it carries (AGENTS.md, Rule 5). Internal changes are not listed, and there
is no entry without a version change. Entries are appended, oldest first,
and never edited.

## libmorph 0.1.0 — 2026-10-14 (`9bf131f`, tag `v0.1.0`)

Initial release, and every change up to `8f19048`. Recorded retroactively
on 2026-10-16; the tag was added on 2026-10-16.

## libmorph-build 0.1.0 — 2026-10-14 (`933c5be`, tag `libmorph-build-v0.1.0`)

Initial release. Recorded retroactively on 2026-10-16; the tag was added on
2026-10-16.

## libmorph 0.2.0 — 2026-10-16 (tag `v0.2.0`)

Audio output device and streaming sources
(`docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md`). The commit is
the one tagged `v0.2.0`: the entry is written in that commit, which cannot
name its own hash.

- **Breaking:** on desktop and the web, the audio stream (voices and
  sources) pauses in winit's `suspended` and plays again in `resumed`, as
  it already did on Android.
- Added `Audio::add_source(Source) -> SourceId`, `remove_source`,
  `format() -> Option<OutputFormat>` and `set_master_volume`, with the
  types `Source` (`Box<dyn FnMut(&mut [f32]) + Send>`), `SourceId` and
  `OutputFormat { sample_rate, channels }`. Without `audio`, they exist and
  do nothing.
- Added the `audio-cpal` feature: `libmorph::cpal`, and
  `Audio::device() -> Option<&impl DeviceTrait>`, `stream_config()` and
  `sample_format()`.
- `stop_all` stops voices only; sources keep running.
- On Android, cpal now comes from libmobile 0.1.1 (`libmobile/cpal`); the
  libmobile version is compatible with 0.1.0.

## libmorph 0.2.1 — 2026-10-20 (tag v0.2.1)

Strictly additive (Rule 5.4), plan
`docs/design/20261016_02_PIPELINE_CACHE_PLAN.md`:

- Added `MorphContext::pipeline_cache() -> Option<&wgpu::PipelineCache>`:
  on Android with Vulkan, a cache kept in `<data_dir>/libmorph/`, loaded
  before `init` and saved after `init`, on suspend and on exit; `None`
  elsewhere. On Android the device is now requested with
  `Features::PIPELINE_CACHE` where the adapter offers it.
