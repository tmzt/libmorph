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

    let event_loop = EventLoop::<Ready>::with_user_event().build()?;
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut handler = Handler::<A> {
        launch: Some(launch),
        proxy: event_loop.create_proxy(),
        display: event_loop.owned_display_handle(),
        window: None,
        shell: None,
        mouse: MouseTouch::default(),
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

struct Handler<A> {
    /// Taken when the GPU comes up.
    launch: Option<Launch>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    proxy: EventLoopProxy<Ready>,
    display: OwnedDisplayHandle,
    window: Option<Arc<Window>>,
    shell: Option<Shell<A>>,
    mouse: MouseTouch,
    error: Option<MorphError>,
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
        let spec = &self
            .launch
            .as_ref()
            .expect("launch is taken after the window")
            .spec;
        let attributes =
            Window::default_attributes().with_title(format!("{} (libmorph)", spec.name));

        #[cfg(not(target_arch = "wasm32"))]
        let attributes = attributes
            .with_inner_size(winit::dpi::PhysicalSize::new(
                spec.geometry.width,
                spec.geometry.height,
            ))
            .with_resizable(false);

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
            let gpu = Gpu::new(instance, Some(&surface), fallback).await?;
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
        let mut ctx = MorphContext::new(gpu, launch.spec, launch.args, crate::Audio::open());
        place(&mut ctx, &window, size);

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
    }
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
        self.resume(event_loop);
        self.check_exit(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(shell) = &mut self.shell {
            shell.detach();
        }
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
                let output = self.mouse.moved(position.x as f32, position.y as f32, size);
                self.mouse(output);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let output = self.mouse.button(state == ElementState::Pressed);
                self.mouse(output);
            }
            WindowEvent::CursorLeft { .. } => {
                let output = self.mouse.left();
                self.mouse(output);
            }
            // Presses only: no releases, no auto-repeat, and no synthetic
            // presses for keys already held when the window gains focus.
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed && !event.repeat => {
                if let (Some(shell), Some(key)) =
                    (&mut self.shell, key_from_winit(&event.logical_key))
                {
                    // winit takes no default action on keys: the status is unused.
                    let _ = shell.key(key);
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
