# libmorph

A unified windowing, graphics and device-emulation shell for
[`wgpu`](https://github.com/gfx-rs/wgpu) applications. Write one `MorphApp`
and run it on:

| Host | Driven by | Notes |
|---|---|---|
| Desktop | `winit` | The window takes the exact pixel size of an emulated phone (`--device iphone_15`, from [libdevice](https://github.com/tmzt/libdevice)). The mouse arrives as touches, and an overlay masks the phone's corners and cutouts. |
| Web (`web`) | `winit` | Full-screen canvas; WebGPU with a WebGL2 fallback. |
| Android (`mobile`) | [libmobile](https://github.com/tmzt/libmobile) | `NativeActivity`, system insets as the safe area; the Gradle project is generated from `build.rs`. |

The wgpu device and queue live for the whole run; only the surface is
recreated across suspend and resume. See [AGENTS.md](AGENTS.md) for the
architecture and [CHANGES.md](CHANGES.md) for planned work.

```rust
use libmorph::{Frame, MorphApp, MorphContext};

struct App;

impl MorphApp for App {
    fn init(_ctx: &mut MorphContext) -> Self {
        App
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        // Record into frame.view with ctx.device() / ctx.queue().
    }
}

fn main() -> Result<(), libmorph::MorphError> {
    libmorph::run::<App>()
}
```

`cargo run --example touch -- --device iphone_15` runs a small demo;
`--morph-help` lists the options.

## Architectural decisions

| Document | Date | Status | Decision |
|---|---|---|---|
| [Camera view and capture](docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md) | 2026-10-14 | Hold | Split the camera into `camera-view` (live preview texture) and `camera-capture` (still photo), with `camera-stream` reserved, replacing `mobile-camera`. Desktop uses nokhwa, the web `getUserMedia`, and Android the NDK camera. The decisions are confirmed but the plan is not approved for implementation. |
