//! Switches the laptop's WiFi to the camera and back, through NetworkManager's nmcli.
//!
//! The camera is an open access point with no station mode, and this laptop's card can
//! only be associated with one network at a time, so the home network has to go while the
//! camera is in use. The camera profile is created in memory only (`save no`) and without
//! autoconnect, so NetworkManager never joins the camera on its own later.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const CAMERA_PREFIX: &str = "LMECHN";
const HOME: &str = "Katze";
const PROFILE: &str = "earcam";
const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli")
        .args(args)
        .output()
        .map_err(|e| format!("nmcli: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "nmcli {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn wifi_device() -> Result<String, String> {
    nmcli(&["-t", "-f", "DEVICE,TYPE", "device"])?
        .lines()
        .find_map(|l| l.strip_suffix(":wifi").map(str::to_owned))
        .ok_or_else(|| "no WiFi device".into())
}

fn active_connection(dev: &str) -> Option<String> {
    let out = nmcli(&["-t", "-f", "GENERAL.CONNECTION", "device", "show", dev]).ok()?;
    out.trim()
        .strip_prefix("GENERAL.CONNECTION:")
        .map(str::to_owned)
}

/// Waits for the camera's SSID to show up in a scan (it takes a few seconds to boot).
fn find_camera(dev: &str, running: &AtomicBool) -> Result<String, String> {
    let deadline = Instant::now() + SCAN_TIMEOUT;
    while running.load(Ordering::Relaxed) {
        // A forced rescan is refused while one is already in flight; the cached list
        // is fine then.
        let list = nmcli(&[
            "-t", "-f", "SSID", "device", "wifi", "list", "ifname", dev, "--rescan", "yes",
        ])
        .or_else(|_| nmcli(&["-t", "-f", "SSID", "device", "wifi", "list", "ifname", dev]))?;
        if let Some(ssid) = list.lines().find(|s| s.starts_with(CAMERA_PREFIX)) {
            return Ok(ssid.to_owned());
        }
        if Instant::now() > deadline {
            return Err(format!(
                "no {CAMERA_PREFIX}-* network found, is the camera on?"
            ));
        }
        thread::sleep(Duration::from_secs(2));
    }
    Err("cancelled".into())
}

/// Joins the camera's network. Blocking, up to a minute while the camera boots.
pub fn connect(running: &AtomicBool) -> Result<(), String> {
    let dev = wifi_device()?;
    if active_connection(&dev).as_deref() == Some(PROFILE) {
        return Ok(());
    }
    eprintln!("searching for {CAMERA_PREFIX}-* on {dev}");
    let ssid = find_camera(&dev, running)?;
    eprintln!("connecting to {ssid}");
    let _ = nmcli(&["connection", "delete", PROFILE]);
    nmcli(&[
        "connection",
        "add",
        "type",
        "wifi",
        "con-name",
        PROFILE,
        "ifname",
        &dev,
        "ssid",
        &ssid,
        "connection.autoconnect",
        "no",
        "ipv6.method",
        "disabled",
        "save",
        "no",
    ])?;
    nmcli(&["--wait", "30", "connection", "up", PROFILE])?;
    eprintln!("connected to {ssid}");
    Ok(())
}

/// Drops the camera profile and brings the home network back up, unless it already is.
pub fn restore() {
    let _ = nmcli(&["connection", "delete", PROFILE]);
    let Ok(dev) = wifi_device() else { return };
    if active_connection(&dev).as_deref() == Some(HOME) {
        return;
    }
    eprintln!("reconnecting to {HOME}");
    if let Err(e) = nmcli(&["--wait", "30", "connection", "up", HOME]) {
        eprintln!("{e}");
    }
}
