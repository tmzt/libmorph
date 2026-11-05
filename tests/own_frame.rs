//! `Owned::FRAME`: which hooks each setting reaches, and what
//! `acquire_frame` says when it has nothing to give. Needs a GPU adapter;
//! without one each test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::atomic::{AtomicUsize, Ordering};

use libmorph::script::{OutDir, Report, Script, ScriptRun};
use libmorph::{
    AcquireError, Frame, GpuError, MorphApp, MorphContext, MorphError, Owned, RenderStatus,
};

/// Clear `frame` to `color` and submit.
fn clear(ctx: &MorphContext, view: &wgpu::TextureView, color: wgpu::Color) {
    let mut encoder = ctx.device().create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    ctx.queue().submit([encoder.finish()]);
}

use libmorph::wgpu;

/// Run `A` over `script`, or report that there is no adapter.
fn run<A: MorphApp>(name: &str, script: &str) -> Option<Report> {
    let dir = std::env::temp_dir().join(format!("libmorph-{name}-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (16, 32);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir),
        overlay: false,
        fallback_adapter: false,
        args: vec![name.into()],
    };
    match libmorph::script::run::<A>(&Script::parse(script).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            None
        }
        result => Some(result.unwrap()),
    }
}

// --- Each setting reaches its own hook, and not the other's ---------------

static MANAGED_RENDERS: AtomicUsize = AtomicUsize::new(0);
static MANAGED_CONSTRUCTS: AtomicUsize = AtomicUsize::new(0);

/// A managed app that also implements `construct_frame`, which libmorph
/// must never call.
struct Managed;

impl MorphApp for Managed {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) -> RenderStatus {
        MANAGED_RENDERS.fetch_add(1, Ordering::SeqCst);
        clear(ctx, frame.view, wgpu::Color::GREEN);
        RenderStatus::Drawn
    }

    fn construct_frame(&mut self, _ctx: &mut MorphContext) {
        MANAGED_CONSTRUCTS.fetch_add(1, Ordering::SeqCst);
    }
}

static OWNING_RENDERS: AtomicUsize = AtomicUsize::new(0);
static OWNING_CONSTRUCTS: AtomicUsize = AtomicUsize::new(0);

/// An owning app that also implements `render`, which libmorph must never
/// call.
struct Owning;

impl MorphApp for Owning {
    const OWNS: Owned = Owned::FRAME;

    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        OWNING_RENDERS.fetch_add(1, Ordering::SeqCst);
        RenderStatus::Skipped
    }

    fn construct_frame(&mut self, ctx: &mut MorphContext) {
        OWNING_CONSTRUCTS.fetch_add(1, Ordering::SeqCst);
        let Ok(frame) = ctx.acquire_frame() else {
            return;
        };
        clear(ctx, &frame.view, wgpu::Color::GREEN);
        ctx.present_frame(frame);
    }
}

#[test]
fn each_setting_reaches_its_own_hook() {
    if run::<Managed>("managed-hooks", "frames 3\n").is_none() {
        return;
    }
    assert_eq!(MANAGED_RENDERS.load(Ordering::SeqCst), 3);
    assert_eq!(
        MANAGED_CONSTRUCTS.load(Ordering::SeqCst),
        0,
        "a managed app never reaches construct_frame"
    );

    if run::<Owning>("owning-hooks", "frames 3\n").is_none() {
        return;
    }
    assert_eq!(OWNING_CONSTRUCTS.load(Ordering::SeqCst), 3);
    assert_eq!(
        OWNING_RENDERS.load(Ordering::SeqCst),
        0,
        "an owning app never reaches render"
    );
}

// --- What acquire_frame says when it has nothing to give ------------------

static ERRORS: std::sync::Mutex<Vec<AcquireError>> = std::sync::Mutex::new(Vec::new());

/// A managed app that asks for the frame anyway.
struct Asking;

impl MorphApp for Asking {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) -> RenderStatus {
        if let Err(error) = ctx.acquire_frame() {
            ERRORS.lock().unwrap().push(error);
        }
        clear(ctx, frame.view, wgpu::Color::BLUE);
        RenderStatus::Drawn
    }
}

#[test]
fn a_managed_app_asking_for_the_frame_is_told_it_is_not_its() {
    if run::<Asking>("asking", "frames 2\n").is_none() {
        return;
    }
    assert_eq!(
        *ERRORS.lock().unwrap(),
        [AcquireError::NotOwned, AcquireError::NotOwned]
    );
}

static HELD: std::sync::Mutex<Vec<AcquireError>> = std::sync::Mutex::new(Vec::new());

/// An owning app that acquires twice without presenting the first.
struct Greedy;

impl MorphApp for Greedy {
    const OWNS: Owned = Owned::FRAME;

    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn construct_frame(&mut self, ctx: &mut MorphContext) {
        let Ok(frame) = ctx.acquire_frame() else {
            return;
        };
        if let Err(error) = ctx.acquire_frame() {
            HELD.lock().unwrap().push(error);
        }
        clear(ctx, &frame.view, wgpu::Color::RED);
        ctx.present_frame(frame);
        // Presented, so the next one is the app's to take again.
        if ctx.acquire_frame().is_err() {
            HELD.lock().unwrap().push(AcquireError::Held);
        }
    }
}

#[test]
fn one_frame_per_construct_frame() {
    if run::<Greedy>("greedy", "frames 2\n").is_none() {
        return;
    }
    assert_eq!(
        *HELD.lock().unwrap(),
        [AcquireError::Held, AcquireError::Held],
        "the second acquire is refused each iteration, and the one after \
         presenting is not"
    );
}
