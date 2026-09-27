//! EarCam: a client for i4season WiFi ear cameras (LMECHN and the other brands sharing the
//! firmware) on Android, Linux and Windows, from one codebase.
//!
//! - [`protocol`] and [`assembler`] turn UDP datagrams into JPEG frames, [`stream`] talks
//!   to the camera and decodes, [`orientation`] turns the probe's gravity vector into a
//!   screen rotation, and [`render`] draws with OpenGL ES.
//! - [`discovery`] decides which WiFi networks might be a camera, [`wifi`] joins one
//!   (and leaves it again), and [`session`] ties joining and streaming together.
//! - [`app`] is the window, shared by all platforms; `main.rs` starts it on the desktop,
//!   `android_main` below on Android.

#[cfg(target_os = "android")]
pub mod android;
pub mod app;
pub mod assembler;
pub mod discovery;
pub mod gestures;
pub mod logger;
pub mod orientation;
pub mod protocol;
pub mod render;
pub mod session;
pub mod signals;
pub mod stream;
pub mod wifi;

/// Entry point of the Android app (a NativeActivity loading `libearcam.so`).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(android: winit::platform::android::activity::AndroidApp) {
    use winit::event_loop::EventLoop;
    use winit::platform::android::EventLoopBuilderExtAndroid;

    logger::init();
    log::info!("EarCam {} starting", env!("CARGO_PKG_VERSION"));
    let event_loop = EventLoop::<app::UserEvent>::with_user_event()
        .with_android_app(android)
        .build()
        .expect("event loop");
    let mut app = app::App::new(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("{e}");
    }
    app.shutdown();
    // winit's event loop cannot be created twice in one process, and Android may reuse
    // the process for the next launch.
    std::process::exit(0);
}
