//! Desktop and web: `winit` owns the loop.
//!
//! Desktop sizes the window to the emulated device and brings the GPU up
//! synchronously. The web injects a full-screen canvas and brings the GPU up
//! in a `spawn_local` task that reports back through an [`EventLoopProxy`].

use std::sync::Arc;

use libdevice::{DeviceSpec, InsetType};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle};
use winit::window::{Window, WindowId};

use super::shell::{FrameStatus, Shell};
use crate::config::Launch;
use crate::gpu::{self, Gpu};
use crate::input::{MouseOutput, MouseTouch, key_from_winit};
#[cfg(target_os = "macos")]
use crate::input::{PadMapping, PadTouch};
use crate::{GpuError, MorphApp, MorphContext, MorphError, SafeArea, Touch, TouchPhase};

/// The GPU, and the surface it was chosen for, once bring-up finishes.
type Boot = Result<(Gpu, wgpu::Surface<'static>), GpuError>;

struct Ready(Boot);

/// Run `A` until its window closes.
///
/// On desktop this parses the command line first (see [`crate::MORPH_ARGS_HELP`];
/// `--morph-help` prints it and returns). On the web it returns at once and
/// the app keeps running in the browser's event loop.
///
/// # Errors
///
/// Bad options, a failed GPU bring-up or a failed window or event loop.
pub fn run<A: MorphApp>() -> Result<(), MorphError> {
    #[cfg(not(target_arch = "wasm32"))]
    let launch = Launch::from_args(std::env::args())?;
    #[cfg(target_arch = "wasm32")]
    let launch = Launch::without_args();

    #[cfg(not(target_arch = "wasm32"))]
    if launch.options.help {
        println!("{}", Launch::help());
        return Ok(());
    }

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = launch.script.clone() {
        return run_script::<A>(launch, &path);
    }

    #[cfg(target_os = "macos")]
    let pad = Pad::new(launch.options.trackpad, &launch.spec);
    let event_loop = EventLoop::<Ready>::with_user_event().build()?;
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut handler = Handler::<A> {
        launch: Some(launch),
        proxy: event_loop.create_proxy(),
        display: event_loop.owned_display_handle(),
        window: None,
        shell: None,
        mouse: MouseTouch::default(),
        #[cfg(target_os = "macos")]
        pad,
        error: None,
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        event_loop.run_app(&mut handler)?;
        handler.error.map_or(Ok(()), Err)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(handler);
        Ok(())
    }
}

/// `--morph-script`: run the script headless instead of opening a window.
/// The script is loaded and checked before any GPU work.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(script_runner), allow(clippy::extra_unused_type_parameters))]
fn run_script<A: MorphApp>(launch: Launch, path: &std::path::Path) -> Result<(), MorphError> {
    #[cfg(script_runner)]
    {
        use crate::script::{OutDir, Script, ScriptRun};
        let script = Script::load(path)?;
        let out = launch
            .out
            .map_or_else(|| OutDir::for_script(path), OutDir::Exact);
        crate::script::run::<A>(
            &script,
            ScriptRun {
                spec: launch.spec,
                out,
                overlay: launch.options.overlay,
                fallback_adapter: launch.options.fallback_adapter,
                args: launch.args,
            },
        )?;
        Ok(())
    }
    #[cfg(not(script_runner))]
    {
        let _ = (launch.out, path);
        Err(crate::script::ScriptError::Disabled.into())
    }
}

struct Handler<A> {
    /// Taken when the GPU comes up.
    launch: Option<Launch>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    proxy: EventLoopProxy<Ready>,
    display: OwnedDisplayHandle,
    window: Option<Arc<Window>>,
    shell: Option<Shell<A>>,
    mouse: MouseTouch,
    #[cfg(target_os = "macos")]
    pad: Pad,
    error: Option<MorphError>,
}

/// Trackpad fingers as touches (macOS).
#[cfg(target_os = "macos")]
struct Pad {
    /// Off with `--morph-trackpad off`; installed with the window.
    enabled: bool,
    reader: Option<super::trackpad::Trackpad>,
    touch: PadTouch,
    /// The cursor, while it is over the surface.
    cursor: Option<(f32, f32)>,
}

#[cfg(target_os = "macos")]
impl Pad {
    fn new(option: crate::config::TrackpadOption, spec: &DeviceSpec) -> Self {
        use crate::config::TrackpadOption;
        let mapping = match option {
            TrackpadOption::Fit | TrackpadOption::Off => PadMapping::Fit,
            TrackpadOption::Physical => PadMapping::Physical {
                px_per_mm: spec.px_per_mm(),
            },
        };
        Self {
            enabled: option != TrackpadOption::Off,
            reader: None,
            touch: PadTouch::new(mapping),
            cursor: None,
        }
    }
}

impl<A: MorphApp> Handler<A> {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: MorphError) {
        log::error!("libmorph: {error}");
        self.error = Some(error);
        self.shutdown(event_loop);
    }

    /// Every way out ends here: one `on_suspend` (see `Shell::shutdown`),
    /// then the surface, the app and the window are dropped.
    fn shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(shell) = self.shell.take() {
            shell.shutdown();
        }
        #[cfg(target_os = "macos")]
        {
            self.pad.reader = None;
        }
        self.window = None;
        event_loop.exit();
    }

    /// Called after every event that reaches the app.
    fn check_exit(&mut self, event_loop: &ActiveEventLoop) {
        if self.shell.as_ref().is_some_and(Shell::exit_requested) {
            self.shutdown(event_loop);
        }
    }

    fn create_window(&self, event_loop: &ActiveEventLoop) -> Result<Arc<Window>, MorphError> {
        let launch = self
            .launch
            .as_ref()
            .expect("launch is taken after the window");
        let spec = &launch.spec;
        let attributes =
            Window::default_attributes().with_title(format!("{} (libmorph)", spec.name));

        #[cfg(not(target_arch = "wasm32"))]
        let attributes = {
            let monitor = event_loop.primary_monitor().map(|monitor| {
                let size = monitor.size();
                (size.width, size.height)
            });
            let (width, height) = window_size(
                (spec.geometry.width, spec.geometry.height),
                monitor,
                launch.options.window_scale,
            );
            attributes
                .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
                .with_resizable(false)
        };

        #[cfg(target_arch = "wasm32")]
        let attributes = {
            use winit::platform::web::WindowAttributesExtWebSys;
            attributes.with_append(true)
        };

        let window = event_loop
            .create_window(attributes)
            .map_err(MorphError::Window)?;
        #[cfg(target_arch = "wasm32")]
        fill_page(&window);
        Ok(Arc::new(window))
    }

    /// Start GPU bring-up for `window`; [`Self::ready`] finishes it.
    fn boot(&mut self, event_loop: &ActiveEventLoop, window: Arc<Window>) {
        let fallback = self
            .launch
            .as_ref()
            .is_some_and(|l| l.options.fallback_adapter);
        let display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle> = Box::new(self.display.clone());
        let boot = async move {
            let instance = gpu::instance(Some(display)).await;
            let surface = instance
                .create_surface(window)
                .map_err(GpuError::CreateSurface)?;
            let gpu = Gpu::new(instance, Some(&surface), fallback, A::limits, false).await?;
            Ok((gpu, surface))
        };

        #[cfg(not(target_arch = "wasm32"))]
        self.ready(event_loop, pollster::block_on(boot));

        #[cfg(target_arch = "wasm32")]
        {
            let _ = event_loop;
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = proxy.send_event(Ready(boot.await));
            });
        }
    }

    fn ready(&mut self, event_loop: &ActiveEventLoop, boot: Boot) {
        let (gpu, surface) = match boot {
            Ok(boot) => boot,
            Err(error) => return self.fail(event_loop, error.into()),
        };
        let (Some(window), Some(launch)) = (self.window.clone(), self.launch.take()) else {
            return;
        };
        let size = window.inner_size();
        let size = (size.width, size.height);
        let mut ctx = MorphContext::new(
            gpu,
            launch.spec,
            launch.args,
            crate::Audio::open(super::audio_setup::<A>()),
        );
        place(&mut ctx, &window, size);
        #[cfg(not(target_arch = "wasm32"))]
        {
            (ctx.data_dir, ctx.cache_dir) = crate::storage::desktop(A::APP_ID);
        }
        #[cfg(all(target_os = "macos", feature = "secure-store"))]
        {
            let namespace = crate::secure_store::namespace(A::SECURE_STORE_ID);
            ctx.secure_store = Some(crate::SecureStore::keychain(&namespace));
            ctx.key_store = crate::Vault::named(&namespace).ok();
        }

        match Shell::<A>::start(ctx, surface, size) {
            Ok(shell) => {
                #[cfg(not(target_arch = "wasm32"))]
                let shell = {
                    let mut shell = shell;
                    if launch.options.overlay {
                        shell.overlay = Some(super::overlay::Overlay::new(&shell.ctx));
                    }
                    shell
                };
                self.shell = Some(shell);
                window.request_redraw();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn touch(&mut self, touch: Touch) {
        if let Some(shell) = &mut self.shell {
            shell.touch(touch);
        }
    }

    fn mouse(&mut self, output: Option<MouseOutput>) {
        let (Some(shell), Some(output)) = (&mut self.shell, output) else {
            return;
        };
        match output {
            MouseOutput::Touch(touch) => shell.touch(touch),
            MouseOutput::Hover(x, y) => shell.hover(x, y),
        }
    }

    /// Deliver the trackpad frames read since the last call.
    #[cfg(target_os = "macos")]
    fn drain_pad(&mut self) {
        let Some(reader) = &self.pad.reader else {
            return;
        };
        let frames = reader.drain();
        let surface = self.surface_size();
        let scale = self.shell.as_ref().map_or((1.0, 1.0), |shell| {
            let device = shell.ctx.device_spec().geometry;
            (
                surface.0 as f32 / device.width as f32,
                surface.1 as f32 / device.height as f32,
            )
        });
        for frame in frames {
            log::debug!("trackpad: {frame:?}");
            let was_active = self.pad.touch.active();
            let touches = self
                .pad
                .touch
                .frame(&frame, self.pad.cursor, surface, scale);
            if !was_active && self.pad.touch.active() {
                let output = self.mouse.interrupt();
                self.mouse(output);
            }
            for touch in touches {
                self.touch(touch);
            }
        }
    }

    /// Cancel every trackpad finger still down.
    #[cfg(target_os = "macos")]
    fn cancel_pad(&mut self) {
        for touch in self.pad.touch.cancel() {
            self.touch(touch);
        }
    }

    /// The surface size the mouse is tested against.
    fn surface_size(&self) -> (u32, u32) {
        self.window.as_ref().map_or((0, 0), |window| {
            let size = window.inner_size();
            (size.width, size.height)
        })
    }
}

/// Bring the context's size-dependent values in line with the window.
fn place(ctx: &mut MorphContext, window: &Window, (width, height): (u32, u32)) {
    if cfg!(target_arch = "wasm32") {
        // The real page: no emulated insets, the browser's own scale.
        ctx.spec.geometry.width = width;
        ctx.spec.geometry.height = height;
        ctx.safe_area = SafeArea::NONE;
        ctx.scale_factor = window.scale_factor() as f32;
    } else {
        ctx.safe_area = emulated_safe_area(&ctx.spec, (width, height));
        // A window smaller than the device shows the same dp layout.
        ctx.scale_factor = ctx.spec.pixel_ratio * width as f32 / ctx.spec.geometry.width as f32;
    }
}

/// The window size, in host physical pixels, for a device of `device`
/// pixels on a display of `monitor` pixels. `Fit` leaves room for the menu
/// bar, Dock and title bar, and never enlarges the device.
#[cfg(not(target_arch = "wasm32"))]
fn window_size(
    (width, height): (u32, u32),
    monitor: Option<(u32, u32)>,
    scale: crate::config::WindowScale,
) -> (u32, u32) {
    let factor = match (scale, monitor) {
        (crate::config::WindowScale::Factor(factor), _) => factor,
        (crate::config::WindowScale::Fit, None) => 1.0,
        (crate::config::WindowScale::Fit, Some((monitor_width, monitor_height))) => 1f32
            .min(0.9 * monitor_width as f32 / width as f32)
            .min(0.85 * monitor_height as f32 / height as f32),
    };
    let scaled = |length: u32| ((length as f32 * factor).round() as u32).max(1);
    (scaled(width), scaled(height))
}

/// The spec's safe area, scaled if the window manager did not grant the
/// device's exact size.
fn emulated_safe_area(spec: &DeviceSpec, (width, height): (u32, u32)) -> SafeArea {
    SafeArea::from(spec.insets(InsetType::SafeDrawing)).scaled(
        width as f32 / spec.geometry.width as f32,
        height as f32 / spec.geometry.height as f32,
    )
}

impl<A: MorphApp> ApplicationHandler<Ready> for Handler<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        crate::system_events::publish(crate::system_events::SystemEvent::Visible(true));
        // Before the first resume there is no shell yet, and the stream is
        // already playing.
        if let Some(shell) = &self.shell {
            shell.ctx.audio.resume();
        }
        self.resume(event_loop);
        self.check_exit(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        crate::system_events::publish(crate::system_events::SystemEvent::Visible(false));
        #[cfg(target_os = "macos")]
        self.cancel_pad();
        if let Some(shell) = &mut self.shell {
            // Voices and sources pause with the app.
            shell.ctx.audio.pause();
            shell.detach();
        }
        self.check_exit(event_loop);
    }

    #[cfg(target_os = "macos")]
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.drain_pad();
        self.check_exit(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, Ready(boot): Ready) {
        self.ready(event_loop, boot);
        self.check_exit(event_loop);
    }

    /// The loop is ending for a reason of its own (e.g. Cmd-Q on macOS).
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(shell) = self.shell.take() {
            shell.shutdown();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.handle_window_event(event_loop, event);
        self.check_exit(event_loop);
    }
}

impl<A: MorphApp> Handler<A> {
    fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let window = match self.window.clone() {
            Some(window) => window,
            None => match self.create_window(event_loop) {
                Ok(window) => {
                    #[cfg(target_os = "macos")]
                    if self.pad.enabled {
                        self.pad.reader = super::trackpad::Trackpad::new(&window);
                        if self.pad.reader.is_none() {
                            log::warn!("libmorph: trackpad touches are unavailable");
                        }
                    }
                    self.window = Some(window.clone());
                    window
                }
                Err(error) => return self.fail(event_loop, error),
            },
        };
        let Some(shell) = &mut self.shell else {
            if self.launch.is_some() {
                self.boot(event_loop, window);
            }
            return;
        };
        if shell.has_surface() {
            return;
        }
        match shell.gpu().instance.create_surface(window.clone()) {
            Ok(surface) => {
                let size = window.inner_size();
                shell.attach(surface, (size.width, size.height));
                window.request_redraw();
            }
            Err(error) => self.fail(event_loop, GpuError::CreateSurface(error).into()),
        }
    }

    fn handle_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.shutdown(event_loop),
            WindowEvent::Resized(size) => {
                let size = (size.width, size.height);
                if let (Some(shell), Some(window)) = (&mut self.shell, &self.window) {
                    shell.resize(size);
                    place(&mut shell.ctx, window, size);
                }
            }
            WindowEvent::RedrawRequested => {
                #[cfg(target_os = "macos")]
                self.drain_pad();
                let (Some(shell), Some(window)) = (&mut self.shell, &self.window) else {
                    return;
                };
                if shell.frame() == FrameStatus::Lost {
                    match shell.gpu().instance.create_surface(window.clone()) {
                        Ok(surface) => {
                            let size = window.inner_size();
                            shell.attach(surface, (size.width, size.height));
                        }
                        Err(error) => {
                            return self.fail(event_loop, GpuError::CreateSurface(error).into());
                        }
                    }
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let size = self.surface_size();
                let (x, y) = (position.x as f32, position.y as f32);
                let output = self.mouse.moved(x, y, size);
                #[cfg(target_os = "macos")]
                let output = {
                    let inside =
                        (0.0..size.0 as f32).contains(&x) && (0.0..size.1 as f32).contains(&y);
                    self.pad.cursor = inside.then_some((x, y));
                    // No hover while trackpad fingers are touches.
                    output.filter(|output| {
                        !(self.pad.touch.active() && matches!(output, MouseOutput::Hover(..)))
                    })
                };
                self.mouse(output);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                // Presses during a trackpad session (three-finger drag) are
                // not a second source of touches.
                #[cfg(target_os = "macos")]
                if pressed && self.pad.touch.active() {
                    return;
                }
                let output = self.mouse.button(pressed);
                self.mouse(output);
            }
            WindowEvent::CursorLeft { .. } => {
                #[cfg(target_os = "macos")]
                {
                    self.pad.cursor = None;
                }
                let output = self.mouse.left();
                self.mouse(output);
            }
            #[cfg(target_os = "macos")]
            WindowEvent::Focused(false) => self.cancel_pad(),
            // Presses only: no releases, no auto-repeat, and no synthetic
            // presses for keys already held when the window gains focus.
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed && !event.repeat => {
                if let Some(shell) = &mut self.shell {
                    if shell.ctx.text_input_active {
                        use winit::keyboard::{Key as WinitKey, NamedKey};
                        match &event.logical_key {
                            WinitKey::Named(NamedKey::Escape) => {
                                shell.ctx.stop_text_input();
                                shell.text_input(crate::TextInputEvent::Dismissed);
                            }
                            WinitKey::Named(NamedKey::Backspace) => {
                                shell.ctx.text_input_value.pop();
                                let text = shell.ctx.text_input_value.clone();
                                shell.text_input(crate::TextInputEvent::Changed(text));
                            }
                            WinitKey::Named(NamedKey::Enter) => {
                                let action = shell.ctx.text_input_options.action;
                                shell.text_input(crate::TextInputEvent::Action(action));
                            }
                            _ => {
                                if let Some(s) = event.text.as_deref() {
                                    shell.ctx.text_input_value.push_str(s);
                                    let text = shell.ctx.text_input_value.clone();
                                    shell.text_input(crate::TextInputEvent::Changed(text));
                                }
                            }
                        }
                    } else if let Some(key) = key_from_winit(&event.logical_key) {
                        // winit takes no default action on keys: the status is unused.
                        let _ = shell.key(key);
                    }
                }
            }
            WindowEvent::TouchpadPressure { pressure, .. } => {
                let output = self.mouse.pressure(pressure);
                self.mouse(output);
            }
            WindowEvent::Touch(touch) => self.touch(Touch {
                id: touch.id,
                phase: match touch.phase {
                    winit::event::TouchPhase::Started => TouchPhase::Started,
                    winit::event::TouchPhase::Moved => TouchPhase::Moved,
                    winit::event::TouchPhase::Ended => TouchPhase::Ended,
                    winit::event::TouchPhase::Cancelled => TouchPhase::Cancelled,
                },
                x: touch.location.x as f32,
                y: touch.location.y as f32,
                pressure: touch.force.map(|force| force.normalized() as f32),
            }),
            _ => {}
        }
    }
}

/// Make the canvas cover the viewport and keep the page from scrolling or
/// turning touches into gestures.
#[cfg(target_arch = "wasm32")]
fn fill_page(window: &Window) {
    use winit::platform::web::WindowExtWebSys;

    let set = |style: web_sys::CssStyleDeclaration, properties: &[(&str, &str)]| {
        for (name, value) in properties {
            let _ = style.set_property(name, value);
        }
    };
    if let Some(body) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
    {
        set(body.style(), &[("margin", "0"), ("overflow", "hidden")]);
    }
    if let Some(canvas) = window.canvas() {
        set(
            canvas.style(),
            &[
                ("position", "fixed"),
                ("inset", "0"),
                ("width", "100vw"),
                ("height", "100vh"),
                ("display", "block"),
                ("touch-action", "none"),
            ],
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::run_script;
    use crate::config::Launch;
    use crate::{Frame, MorphApp, MorphContext};

    struct Never;

    impl MorphApp for Never {
        fn init(_ctx: &mut MorphContext) -> Self {
            unreachable!("no GPU work before the script is loaded")
        }

        fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}
    }

    #[cfg(not(script_runner))]
    #[test]
    fn script_without_the_feature_is_disabled() {
        let launch = Launch::from_args(["app", "--morph-script", "x.morph"]).unwrap();
        let error = run_script::<Never>(launch, std::path::Path::new("x.morph")).unwrap_err();
        assert!(matches!(
            error,
            crate::MorphError::Script(crate::script::ScriptError::Disabled)
        ));
    }

    #[cfg(script_runner)]
    #[test]
    fn a_bad_script_fails_before_any_gpu_work() {
        let dir = std::env::temp_dir().join(format!("libmorph-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.morph");
        std::fs::write(&path, "frames 1\nscreenshot a\nscreenshot a\n").unwrap();
        let launch = Launch::from_args(["app"]).unwrap();
        let error = run_script::<Never>(launch, &path).unwrap_err();
        assert!(matches!(
            error,
            crate::MorphError::Script(crate::script::ScriptError::Parse { line: 3, .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod window_size_tests {
    use super::window_size;
    use crate::config::WindowScale;

    const S24: (u32, u32) = (1080, 2340);

    #[test]
    fn fit_shrinks_a_tall_device_onto_a_laptop_display() {
        // A 14-inch MacBook Pro.
        assert_eq!(
            window_size(S24, Some((3024, 1964)), WindowScale::Fit),
            (770, 1669)
        );
    }

    #[test]
    fn fit_never_enlarges() {
        assert_eq!(window_size(S24, Some((7680, 4320)), WindowScale::Fit), S24);
        assert_eq!(window_size(S24, None, WindowScale::Fit), S24);
    }

    #[test]
    fn a_factor_is_exact() {
        let display = Some((3024, 1964));
        assert_eq!(window_size(S24, display, WindowScale::Factor(1.0)), S24);
        assert_eq!(
            window_size(S24, display, WindowScale::Factor(0.5)),
            (540, 1170)
        );
    }
}
