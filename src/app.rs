//! The application: one window, the camera image rotated upright inside a circle on
//! black. The same winit handler runs on Android, Linux and Windows.
//!
//! Frames go from the stream thread through the [`Exchange`] straight to the GL code; the
//! window redraws once per camera frame and never at display rate. The one exception is a
//! touch gesture in progress, which redraws per touch event.

use std::num::NonZeroU32;
use std::sync::Arc;
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
use winit::event::{ElementState, KeyEvent, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

use crate::gestures::{Gestures, Phase};
use crate::orientation::{DEADBAND, UPRIGHT_OFFSET, angle_degrees, deadband, factor_for, smooth};
use crate::render::Renderer;
use crate::session::{self, Session};
use crate::stream::{Exchange, Slot};

/// What the camera streams at; the display needs nothing faster.
pub const CAMERA_FPS: f32 = 17.0;

pub enum UserEvent {
    Frame,
    Failed(String),
    Quit,
}

struct Gl {
    // Field order is drop order: GL objects before the context, the context before the window.
    renderer: Renderer,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    exchange: Arc<Exchange>,
    session: Option<Session>,
    gl: Option<Gl>,
    /// The frame on screen, kept to redraw after the GL context is recreated.
    front: Slot,
    angle: f32,
    target: f32,
    deadband: f32,
    last_draw: Option<Instant>,
    gestures: Gestures,
    /// Held while the camera image is on screen.
    #[cfg(target_os = "android")]
    screen_on: Option<crate::android::ScreenOn>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> App {
        App {
            proxy,
            exchange: Arc::new(Exchange::default()),
            session: None,
            gl: None,
            front: Slot::default(),
            angle: UPRIGHT_OFFSET,
            target: UPRIGHT_OFFSET,
            deadband: DEADBAND,
            last_draw: None,
            gestures: Gestures::default(),
            #[cfg(target_os = "android")]
            screen_on: None,
        }
    }

    /// Closes the window, then stops streaming and leaves the camera's network.
    pub fn shutdown(&mut self) {
        self.gl = None;
        #[cfg(target_os = "android")]
        {
            self.screen_on = None;
        }
        if let Some(session) = self.session.take() {
            session.stop();
        }
    }

    fn start_session(&mut self) {
        if self.session.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        self.session = Some(Session::start(self.exchange.clone(), move |event| {
            let _ = proxy.send_event(match event {
                session::Event::Frame => UserEvent::Frame,
                session::Event::Failed(e) => UserEvent::Failed(e),
            });
        }));
    }

    fn create_gl(&self, event_loop: &ActiveEventLoop) -> Result<Gl, Box<dyn std::error::Error>> {
        let attrs = Window::default_attributes().with_title("EarCam");
        #[cfg(not(target_os = "android"))]
        let attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(event_loop, ConfigTemplateBuilder::new(), |configs| {
                // No multisampling: every pixel comes from a shader anyway.
                configs
                    .min_by_key(|c| c.num_samples())
                    .expect("no GL config")
            })?;
        let window = window.ok_or("no window")?;
        #[cfg(not(target_os = "android"))]
        window.set_cursor_visible(false);

        let context = create_context(&config, window.window_handle()?.as_raw())?;
        let surface_attrs = window.build_surface_attributes(Default::default())?;
        let display = config.display();
        let surface = unsafe { display.create_window_surface(&config, &surface_attrs)? };
        let context = context.make_current(&surface)?;
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));

        let gl =
            unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
        let mut renderer = Renderer::new(gl)?;
        // A fresh context has no texture; restore the last frame instead of going black.
        renderer.upload(&self.front);
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
            if let Some(roll) = angle_degrees(g) {
                self.target = UPRIGHT_OFFSET - roll;
                self.deadband = deadband(g);
            }
        }
        let now = Instant::now();
        let dt_ms = self
            .last_draw
            .map_or(0.0, |t| (now - t).as_secs_f32() * 1e3)
            .min(100.0);
        self.last_draw = Some(now);
        self.angle = smooth(self.angle, self.target, factor_for(dt_ms), self.deadband);

        let size = gl.window.inner_size();
        gl.renderer
            .draw(size.width, size.height, self.angle, self.gestures.zoom);
        let _ = gl.surface.swap_buffers(&gl.context);
    }

    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        // Android: finishing the activity ends the event loop through its lifecycle.
        #[cfg(target_os = "android")]
        crate::android::finish();
        #[cfg(not(target_os = "android"))]
        event_loop.exit();
        let _ = event_loop;
    }
}

/// GLES 3.0 where available (Android, Linux, most Windows drivers), so the shaders run as
/// written; otherwise desktop GL 3.3 core, which compiles the same shader source.
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
        if self.gl.is_none() {
            match self.create_gl(event_loop) {
                Ok(gl) => {
                    #[cfg(target_os = "android")]
                    {
                        declare_frame_rate(event_loop);
                        self.screen_on = crate::android::ScreenOn::acquire();
                    }
                    gl.window.request_redraw();
                    self.gl = Some(gl);
                }
                Err(e) => {
                    log::error!("OpenGL setup failed: {e}");
                    event_loop.exit();
                    return;
                }
            }
        }
        self.start_session();
    }

    /// Android only: the app left the foreground and its window is gone.
    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.shutdown();
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.shutdown();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Frame => {
                if let Some(gl) = &self.gl {
                    gl.window.request_redraw();
                }
            }
            UserEvent::Failed(e) => {
                log::error!("{e}");
                // Android keeps the black screen and says so; the desktop gives up.
                // On a fresh thread: a Toast needs a Java Looper, which must not be
                // added to the event loop's thread.
                #[cfg(target_os = "android")]
                std::thread::spawn(|| {
                    crate::android::toast("Keine Kamera gefunden.\nIst sie eingeschaltet?")
                });
                #[cfg(not(target_os = "android"))]
                event_loop.exit();
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
                Key::Named(NamedKey::Escape | NamedKey::GoBack | NamedKey::BrowserBack) => {
                    self.quit(event_loop)
                }
                Key::Character(c) if c.eq_ignore_ascii_case("q") => self.quit(event_loop),
                _ => {}
            },
            WindowEvent::Touch(touch) => {
                let Some(gl) = &self.gl else { return };
                let size = gl.window.inner_size();
                let phase = match touch.phase {
                    TouchPhase::Started => Phase::Down,
                    TouchPhase::Moved => Phase::Move,
                    TouchPhase::Ended | TouchPhase::Cancelled => Phase::Up,
                };
                let (x, y) = (touch.location.x as f32, touch.location.y as f32);
                let size = (size.width as f32, size.height as f32);
                if self
                    .gestures
                    .touch(touch.id, phase, x, y, Instant::now(), size)
                {
                    gl.window.request_redraw();
                }
            }
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

/// Tells the display the content changes at the camera's rate, so it can drop to its
/// lowest refresh rate. (Only reads from android-activity; see `android.rs` for why the
/// screen is kept on through JNI instead of window flags.)
#[cfg(target_os = "android")]
fn declare_frame_rate(event_loop: &ActiveEventLoop) {
    use winit::platform::android::ActiveEventLoopExtAndroid;
    if let Some(window) = event_loop.android_app().native_window() {
        let fixed = ndk::native_window::FrameRateCompatibility::FixedSource;
        if let Err(e) = window.set_frame_rate(CAMERA_FPS, fixed) {
            log::warn!("frame rate hint: {e}");
        }
    }
}
