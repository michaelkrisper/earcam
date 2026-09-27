//! Switches the WiFi to the camera on start and back to the previous network on quit.
//!
//! The camera is an open access point with no station mode, and a laptop's WiFi card is
//! associated with one network at a time, so the regular network has to go while the
//! camera is in use. The camera connection is never saved: on Linux it is an in-memory
//! NetworkManager profile, on Windows a temporary WLAN profile, so neither system joins
//! the camera on its own later.
//!
//! The network to return to is whichever one was connected at start, or [`HOME_FALLBACK`]
//! if there was none (e.g. EarCam was started while already on the camera).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Backend;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Backend;

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("EarCam Desktop supports Linux (NetworkManager) and Windows");

/// Covers "LMECHN-59F77" and every sibling unit of the brand.
pub const CAMERA_PREFIX: &str = "LMECHN";
/// Returned to on quit when no other network was connected at start.
pub const HOME_FALLBACK: &str = "Katze";

/// How long to wait for the camera's network to appear; it takes a few seconds to boot.
const SCAN_TIMEOUT: Duration = Duration::from_secs(60);
const SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// The network an interface is connected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The saved profile, which is what [`Link::restore`] reconnects by.
    pub profile: String,
    pub ssid: String,
}

pub fn is_camera(ssid: &str) -> bool {
    ssid.starts_with(CAMERA_PREFIX)
}

/// The first camera among the visible networks.
pub fn pick_camera<'a>(ssids: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    ssids.into_iter().find(|s| is_camera(s))
}

/// Joins the camera and remembers where to return to.
#[derive(Default)]
pub struct Link {
    previous: Mutex<Option<String>>,
}

impl Link {
    pub fn new() -> Self {
        Self::default()
    }

    /// Joins the camera's network and returns its SSID. Blocks while waiting for the
    /// camera to appear, up to a minute; gives up early once `running` goes false.
    pub fn connect(&self, running: &AtomicBool) -> Result<String, String> {
        let backend = Backend::open()?;
        match backend.current()? {
            Some(c) if is_camera(&c.ssid) => return Ok(c.ssid),
            Some(c) => *self.previous.lock().unwrap() = Some(c.profile),
            None => {}
        }

        eprintln!("searching for {CAMERA_PREFIX}-*");
        let deadline = Instant::now() + SCAN_TIMEOUT;
        let ssid = loop {
            let visible = backend.scan()?;
            if let Some(ssid) = pick_camera(visible.iter().map(String::as_str)) {
                break ssid.to_owned();
            }
            if Instant::now() > deadline {
                return Err(format!(
                    "no {CAMERA_PREFIX}-* network found, is the camera on?"
                ));
            }
            let wake = Instant::now() + SCAN_INTERVAL;
            while Instant::now() < wake {
                if !running.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                thread::sleep(Duration::from_millis(100));
            }
        };

        eprintln!("connecting to {ssid}");
        backend.join(&ssid)?;
        eprintln!("connected to {ssid}");
        Ok(ssid)
    }

    /// Leaves the camera and reconnects the network that was active before [`connect`],
    /// unless it already is. Safe to call whether or not `connect` got anywhere.
    ///
    /// [`connect`]: Link::connect
    pub fn restore(&self) {
        let target = self.previous.lock().unwrap().take();
        let target = target.as_deref().unwrap_or(HOME_FALLBACK);
        let backend = match Backend::open() {
            Ok(b) => b,
            Err(e) => return eprintln!("{e}"),
        };
        backend.leave_camera();
        if backend
            .current()
            .ok()
            .flatten()
            .is_some_and(|c| c.profile == target)
        {
            return;
        }
        eprintln!("reconnecting to {target}");
        if let Err(e) = backend.activate(target) {
            eprintln!("{e}");
        }
    }
}

/// A WLAN profile for an open network, as Windows' `WlanConnect` expects it.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn open_profile_xml(ssid: &str) -> String {
    let name = ssid
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
    <name>{name}</name>
    <SSIDConfig><SSID><name>{name}</name></SSID></SSIDConfig>
    <connectionType>ESS</connectionType>
    <connectionMode>manual</connectionMode>
    <MSM><security><authEncryption>
        <authentication>open</authentication>
        <encryption>none</encryption>
        <useOneX>false</useOneX>
    </authEncryption></security></MSM>
</WLANProfile>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_first_camera_network() {
        let visible = ["Katze", "", "LMECHN-59F77", "LMECHN-0000"];
        assert_eq!(pick_camera(visible), Some("LMECHN-59F77"));
        assert_eq!(pick_camera(["Katze", "lmechn-lowercase"]), None);
    }

    #[test]
    fn profile_xml_is_open_and_manual() {
        let xml = open_profile_xml("LMECHN-59F77");
        assert!(xml.contains("<name>LMECHN-59F77</name>"));
        assert!(xml.contains("<authentication>open</authentication>"));
        assert!(xml.contains("<encryption>none</encryption>"));
        assert!(xml.contains("<connectionMode>manual</connectionMode>"));
    }

    #[test]
    fn profile_xml_escapes_ssid() {
        let xml = open_profile_xml("a<b>&\"c'");
        assert!(xml.contains("<name>a&lt;b&gt;&amp;&quot;c&apos;</name>"));
    }
}
