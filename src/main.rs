//! EarCam for the desktop: joins the camera's WiFi on start, shows the live image rotated
//! upright inside a circle on black, fullscreen, and goes back to the previous network on
//! quit (q, Escape, closing the window, Ctrl+C or a termination signal).

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentContext, PossiblyCurrentContext,
    Version,
};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Fullscreen, Window, WindowId};

use earcam_desktop::orientation::{DEADBAND, UPRIGHT_OFFSET, angle_degrees, factor_for, smooth};
use earcam_desktop::render::Renderer;
use earcam_desktop::signals::{self, QuitSignals};
use earcam_desktop::stream::{self, Camera, Exchange, Slot};
use earcam_desktop::wifi::Link;

enum UserEvent {
    Frame,
    Quit,
}

struct Gl {
    // Field order is drop order: GL objects before the context, the context before the window.
    renderer: Renderer,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
}

struct App {
    exchange: Arc<Exchange>,
    gl: Option<Gl>,
    front: Slot,
    angle: f32,
    target: f32,
    last_draw: Option<Instant>,
}

impl App {
    fn create_gl(&self, event_loop: &ActiveEventLoop) -> Result<Gl, Box<dyn std::error::Error>> {
        let attrs = Window::default_attributes()
            .with_title("EarCam")
            .with_fullscreen(Some(Fullscreen::Borderless(None)));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(event_loop, ConfigTemplateBuilder::new(), |configs| {
                // No multisampling: every pixel comes from a shader anyway.
                configs
                    .min_by_key(|c| c.num_samples())
                    .expect("no GL config")
            })?;
        let window = window.ok_or("no window")?;
        window.set_cursor_visible(false);

        let context = create_context(&config, window.window_handle()?.as_raw())?;
        let surface_attrs = window.build_surface_attributes(Default::default())?;
        let display = config.display();
        let surface = unsafe { display.create_window_surface(&config, &surface_attrs)? };
        let context = context.make_current(&surface)?;
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));

        let gl =
            unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
        let renderer = Renderer::new(gl)?;
        Ok(Gl {
            renderer,
            surface,
            context,
            window,
        })
    }

    fn draw(&mut self) {
        let Some(gl) = &mut self.gl else { return };
        if self.exchange.take(&mut self.front) {
            gl.renderer.upload(&self.front);
            let g = self.front.gravity;
            self.target = UPRIGHT_OFFSET - angle_degrees(g.y, g.z);
        }
        let now = Instant::now();
        let dt_ms = self
            .last_draw
            .map_or(0.0, |t| (now - t).as_secs_f32() * 1e3)
            .min(100.0);
        self.last_draw = Some(now);
        self.angle = smooth(self.angle, self.target, factor_for(dt_ms), DEADBAND);

        let size = gl.window.inner_size();
        gl.renderer.draw(size.width, size.height, self.angle);
        let _ = gl.surface.swap_buffers(&gl.context);
    }
}

/// GLES 3.0 where available (Linux, most Windows drivers), so the shaders run exactly as
/// on Android; otherwise desktop GL 3.3 core, which compiles the same shader source.
fn create_context(
    config: &Config,
    window: RawWindowHandle,
) -> Result<NotCurrentContext, glutin::error::Error> {
    let display = config.display();
    let gles = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
        .build(Some(window));
    let gl = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .with_profile(GlProfile::Core)
        .build(Some(window));
    unsafe {
        display
            .create_context(config, &gles)
            .or_else(|_| display.create_context(config, &gl))
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gl.is_some() {
            return;
        }
        match self.create_gl(event_loop) {
            Ok(gl) => self.gl = Some(gl),
            Err(e) => {
                eprintln!("OpenGL setup failed: {e}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            // Drawn once per camera frame, never at display rate.
            UserEvent::Frame => {
                if let Some(gl) = &self.gl {
                    gl.window.request_redraw();
                }
            }
            UserEvent::Quit => event_loop.exit(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Character(c) if c.eq_ignore_ascii_case("q") => event_loop.exit(),
                _ => {}
            },
            WindowEvent::Resized(size) => {
                if let Some(gl) = &self.gl {
                    if let (Some(w), Some(h)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    {
                        gl.surface.resize(&gl.context, w, h);
                    }
                    gl.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}

fn main() {
    // Before anything spawns a thread, so every thread inherits the blocked signals.
    let signals = QuitSignals::block();
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();
    signals.spawn(move || {
        let _ = proxy.send_event(UserEvent::Quit);
    });

    let running = Arc::new(AtomicBool::new(true));
    let exchange = Arc::new(Exchange::default());
    let link = Arc::new(Link::new());
    let worker = {
        let (running, exchange, link) = (running.clone(), exchange.clone(), link.clone());
        let proxy = event_loop.create_proxy();
        thread::spawn(move || {
            let result = link.connect(&running).and_then(|_| {
                stream::run(&Camera::default(), &running, &exchange, || {
                    let _ = proxy.send_event(UserEvent::Frame);
                })
                .map_err(|e| format!("stream ended: {e}"))
            });
            if let Err(e) = result {
                eprintln!("{e}");
                let _ = proxy.send_event(UserEvent::Quit);
            }
        })
    };

    let mut app = App {
        exchange,
        gl: None,
        front: Slot::default(),
        angle: UPRIGHT_OFFSET,
        target: UPRIGHT_OFFSET,
        last_draw: None,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("{e}");
    }
    // Close the window first: switching the WiFi back can take a few seconds.
    drop(app);

    running.store(false, Ordering::Relaxed);
    let _ = worker.join();
    link.restore();
    signals::finished();
}
