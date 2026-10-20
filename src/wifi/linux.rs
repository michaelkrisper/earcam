//! NetworkManager backend, through `nmcli`. Needs no root on a desktop session: polkit
//! lets the logged-in user manage WiFi connections.

use std::process::Command;

use super::Connection;
use crate::discovery::{Network, parse_mac};

/// Name of the in-memory profile for the camera.
const PROFILE: &str = "earcam";

pub struct Backend {
    device: String,
}

impl Backend {
    pub fn open() -> Result<Backend, String> {
        let out = nmcli(&["-t", "-f", "DEVICE,TYPE", "device"])?;
        let device = out
            .lines()
            .map(split_terse)
            .find(|f| f.len() == 2 && f[1] == "wifi")
            .map(|mut f| f.swap_remove(0))
            .ok_or("no WiFi device")?;
        Ok(Backend { device })
    }

    pub fn current(&self) -> Result<Option<Connection>, String> {
        let out = nmcli(&[
            "-t",
            "-f",
            "GENERAL.CONNECTION",
            "device",
            "show",
            &self.device,
        ])?;
        let Some(profile) = out.trim().strip_prefix("GENERAL.CONNECTION:").map(unescape) else {
            return Ok(None);
        };
        if profile.is_empty() || profile == "--" {
            return Ok(None);
        }
        let ssid = nmcli(&["-g", "802-11-wireless.ssid", "connection", "show", &profile])?;
        Ok(Some(Connection {
            ssid: unescape(ssid.trim()),
            profile,
        }))
    }

    /// Visible networks, after asking for a fresh scan.
    pub fn scan(&self) -> Result<Vec<Network>, String> {
        let list = [
            "-t",
            "-f",
            "SSID,BSSID,SECURITY",
            "device",
            "wifi",
            "list",
            "ifname",
            &self.device,
        ];
        // A forced rescan is refused while one is already running; the cached list is
        // fine then.
        let out = nmcli(&[&list[..], &["--rescan", "yes"]].concat()).or_else(|_| nmcli(&list))?;
        Ok(out.lines().filter_map(parse_scan_line).collect())
    }

    pub fn join(&self, ssid: &str) -> Result<(), String> {
        let _ = nmcli(&["connection", "delete", PROFILE]);
        nmcli(&[
            "connection",
            "add",
            "type",
            "wifi",
            "con-name",
            PROFILE,
            "ifname",
            &self.device,
            "ssid",
            ssid,
            "connection.autoconnect",
            "no",
            "ipv6.method",
            "disabled",
            "save",
            "no",
        ])?;
        nmcli(&["--wait", "30", "connection", "up", PROFILE]).map(drop)
    }

    pub fn leave_camera(&self) {
        let _ = nmcli(&["connection", "delete", PROFILE]);
    }

    pub fn activate(&self, profile: &str) -> Result<(), String> {
        nmcli(&["--wait", "30", "connection", "up", profile]).map(drop)
    }
}

fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli")
        .args(args)
        // Parsed output must not depend on the user's language.
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("nmcli: {e} (is NetworkManager installed?)"))?;
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

/// Splits one line of `nmcli -t` output into fields. Terse mode separates fields with ':'
/// and escapes ':' and '\' inside values with a backslash.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => fields.last_mut().unwrap().extend(chars.next()),
            ':' => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

/// One line of `nmcli -t -f SSID,BSSID,SECURITY device wifi list`. SECURITY is empty
/// (or `--`) for an open network, else e.g. `WPA2`.
fn parse_scan_line(line: &str) -> Option<Network> {
    let [ssid, bssid, security]: [String; 3] = split_terse(line).try_into().ok()?;
    Some(Network {
        ssid,
        bssid: parse_mac(&bssid),
        open: security.is_empty() || security == "--",
    })
}

/// A single `nmcli -t` value.
fn unescape(value: &str) -> String {
    split_terse(value).join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_terse_handles_escapes() {
        assert_eq!(split_terse("wlp3s0:wifi"), ["wlp3s0", "wifi"]);
        assert_eq!(split_terse(r"a\:b:c\\d"), [r"a:b", r"c\d"]);
        assert_eq!(split_terse("lo:"), ["lo", ""]);
    }

    #[test]
    fn parses_scan_lines() {
        let camera = parse_scan_line(r"LMECHN-59F77:82\:77\:8A\:01\:58\:74:").unwrap();
        assert_eq!(camera.ssid, "LMECHN-59F77");
        assert_eq!(camera.bssid, parse_mac("82:77:8a:01:58:74"));
        assert!(camera.open);
        let home = parse_scan_line(r"HomeNetwork:E4\:E1\:30\:00\:00\:02:WPA2").unwrap();
        assert!(!home.open);
        assert!(parse_scan_line("garbage").is_none());
    }

    #[test]
    fn unescape_keeps_colons_in_ssid() {
        assert_eq!(unescape(r"Cafe\: Guest"), "Cafe: Guest");
        assert_eq!(unescape("LMECHN-59F77"), "LMECHN-59F77");
    }
}
