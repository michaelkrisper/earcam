//! One visit to the camera: join its network, stream, and on [`Session::stop`] undo it.
//!
//! The desktop runs one session per launch; Android one per time the app is in the
//! foreground.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use crate::stream::{self, Camera, Exchange};
use crate::wifi::Link;

pub enum Event {
    /// A new frame is in the exchange.
    Frame,
    /// No camera, or the stream broke; the session is over.
    Failed(String),
}

pub struct Session {
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    link: Arc<Link>,
}

impl Session {
    /// Starts connecting and streaming on a background thread.
    pub fn start(exchange: Arc<Exchange>, notify: impl Fn(Event) + Send + 'static) -> Session {
        let running = Arc::new(AtomicBool::new(true));
        let link = Arc::new(Link::new());
        let worker = {
            let (running, link) = (running.clone(), link.clone());
            thread::Builder::new()
                .name("earcam-stream".into())
                .spawn(move || {
                    let result = link.connect(&running).and_then(|route| {
                        stream::run(&Camera::default(), &route, &running, &exchange, || {
                            notify(Event::Frame)
                        })
                        .map_err(|e| format!("stream ended: {e}"))
                    });
                    // Errors after stop() are just the cancellation.
                    if let Err(e) = result
                        && running.load(Ordering::Relaxed)
                    {
                        notify(Event::Failed(e));
                    }
                })
                .expect("spawn stream thread")
        };
        Session {
            running,
            worker: Some(worker),
            link,
        }
    }

    /// Stops streaming and leaves the camera's network. Blocks for up to a second or two,
    /// longer on the desktop while the previous WiFi reconnects.
    pub fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.link.restore();
    }
}
