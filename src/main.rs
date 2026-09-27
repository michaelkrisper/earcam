//! EarCam on the desktop: joins the camera's WiFi on start, shows the live image
//! fullscreen, and reconnects the previous network on quit (q, Escape, closing the window,
//! Ctrl+C or a termination signal).

#[cfg(not(target_os = "android"))]
fn main() {
    use earcam::app::{App, UserEvent};
    use earcam::signals::{self, QuitSignals};
    use winit::event_loop::EventLoop;

    earcam::logger::init();
    // Before anything spawns a thread, so every thread inherits the blocked signals.
    let signals = QuitSignals::block();
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();
    signals.spawn(move || {
        let _ = proxy.send_event(UserEvent::Quit);
    });

    let mut app = App::new(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("{e}");
    }
    app.shutdown();
    signals::finished();
}

/// Android starts the library's `android_main` instead.
#[cfg(target_os = "android")]
fn main() {}
