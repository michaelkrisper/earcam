//! Switching a laptop's WiFi to the camera and back, on top of a platform [`Backend`]
//! (NetworkManager on Linux, the WLAN API on Windows).

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use super::linux::Backend;
#[cfg(windows)]
use super::windows::Backend;

use super::HOME_WIFI_ENV;
use crate::discovery::{self, has_known_prefix};
use crate::protocol::DeviceInfo;
use crate::stream::{self, Camera, Route};

/// How long to look for a camera; it takes a few seconds to boot.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(60);
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
/// How long a freshly joined network gets to answer as a camera.
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(4);
const RESTORE_ATTEMPTS: u32 = 3;

/// Joins the camera and remembers where to return to.
#[derive(Default)]
pub struct Link {
    previous: Mutex<Option<String>>,
}

impl Link {
    pub fn new() -> Self {
        Self::default()
    }

    /// Finds and joins a camera; blocks up to a minute while waiting for one to appear,
    /// and gives up early once `running` goes false.
    pub fn connect(&self, running: &AtomicBool) -> Result<Route, String> {
        let route = Route::default();
        let camera = Camera::default();
        let backend = Backend::open()?;

        // Already on a camera's network (joined by hand, or left over from a crash)?
        let current = backend.current()?;
        if let Some(info) = stream::probe(&camera, &route, Duration::from_millis(800)) {
            found(&info);
            return Ok(route);
        }
        *self.previous.lock().unwrap() = current.map(|c| c.profile);

        log::info!("searching for a camera");
        let deadline = Instant::now() + SEARCH_TIMEOUT;
        // Networks that were joined and did not answer. Known brands are retried: such a
        // camera may just not have finished booting.
        let mut not_cameras = HashSet::new();
        while Instant::now() < deadline {
            for candidate in discovery::candidates(&backend.scan()?) {
                if not_cameras.contains(&candidate.ssid) {
                    continue;
                }
                if !running.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                log::info!("trying {}", candidate.ssid);
                if let Err(e) = backend.join(&candidate.ssid) {
                    log::warn!("{e}");
                    continue;
                }
                if let Some(info) = stream::probe(&camera, &route, IDENTIFY_TIMEOUT) {
                    found(&info);
                    return Ok(route);
                }
                log::info!("{} does not answer like a camera", candidate.ssid);
                if !has_known_prefix(&candidate.ssid) {
                    not_cameras.insert(candidate.ssid.clone());
                }
            }
            sleep_unless_cancelled(SCAN_INTERVAL, running)?;
        }
        Err("no camera found, is it switched on?".into())
    }

    /// Leaves the camera and reconnects the network that was active before [`connect`],
    /// unless it already is. Safe to call whether or not `connect` got anywhere.
    ///
    /// [`connect`]: Link::connect
    pub fn restore(&self) {
        let target = (self.previous.lock().unwrap().take())
            .or_else(|| std::env::var(HOME_WIFI_ENV).ok().filter(|s| !s.is_empty()));
        let backend = match Backend::open() {
            Ok(b) => b,
            Err(e) => return log::warn!("{e}"),
        };
        backend.leave_camera();
        let Some(target) = target else { return };
        let target = target.as_str();
        if backend
            .current()
            .ok()
            .flatten()
            .is_some_and(|c| c.profile == target)
        {
            return;
        }
        // Right after leaving the camera the first association sometimes fails, and
        // NetworkManager then blocks autoconnect for minutes; a short retry gets through.
        for attempt in 1..=RESTORE_ATTEMPTS {
            log::info!("reconnecting to {target}");
            match backend.activate(target) {
                Ok(()) => return,
                Err(e) if attempt < RESTORE_ATTEMPTS => {
                    log::info!("{e}");
                    thread::sleep(Duration::from_secs(2));
                }
                Err(e) => log::warn!("{e}"),
            }
        }
    }
}

fn found(info: &DeviceInfo) {
    log::info!("camera {}: {info}", info.ssid);
}

fn sleep_unless_cancelled(duration: Duration, running: &AtomicBool) -> Result<(), String> {
    let wake = Instant::now() + duration;
    while Instant::now() < wake {
        if !running.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
