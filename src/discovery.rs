//! Which WiFi networks might be a camera.
//!
//! The same i4season firmware ships under many brands, each with its own network name, so
//! the name alone cannot identify a camera. What they share is visible in a scan and on the
//! wire:
//!
//! - an **open** access point (no password),
//! - a BSSID with the **locally administered** bit set (the camera firmware makes
//!   up its MAC rather than using one from a vendor range, as on the LMECHN
//!   unit), or else a network name from [`KNOWN_PREFIXES`],
//! - and, once joined, an answer to GetDeviceInfo at 192.168.1.1, which is the actual
//!   proof. Candidates that do not answer are left again.
//!
//! On Android the system's network request takes a single pattern, which is why the
//! BSSID rule exists: "open, locally administered BSSID" covers every brand in one request.

/// Network name prefixes of known camera brands, matched case-insensitively. Candidates
/// with these names are tried first. `LMECHN` is verified on hardware; the others are
/// names other brands of the same camera family use.
pub const KNOWN_PREFIXES: &[&str] = &["LMECHN", "SUNUO", "SUEAR", "inskam"];

/// A network seen in a WiFi scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    pub bssid: Option<[u8; 6]>,
    /// No password: neither WEP nor WPA.
    pub open: bool,
}

pub fn has_known_prefix(ssid: &str) -> bool {
    KNOWN_PREFIXES.iter().any(|p| {
        ssid.get(..p.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(p))
    })
}

/// The "locally administered" bit of a MAC address: set on made-up addresses, clear on
/// every address from a manufacturer's range.
pub fn is_locally_administered(mac: &[u8; 6]) -> bool {
    mac[0] & 0x02 != 0
}

/// Networks worth joining to look for a camera, most likely first: known brand names,
/// then other open networks with a locally administered BSSID. Each SSID appears once.
pub fn candidates(networks: &[Network]) -> Vec<&Network> {
    let mut known = Vec::new();
    let mut other = Vec::new();
    for n in networks.iter().filter(|n| n.open && !n.ssid.is_empty()) {
        if known
            .iter()
            .chain(&other)
            .any(|c: &&Network| c.ssid == n.ssid)
        {
            continue;
        }
        if has_known_prefix(&n.ssid) {
            known.push(n);
        } else if n.bssid.as_ref().is_some_and(is_locally_administered) {
            other.push(n);
        }
    }
    known.extend(other);
    known
}

/// Parses `82:0a:0b:0c:0d:01` (or with dashes).
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut mac = [0u8; 6];
    let mut parts = s.split([':', '-']);
    for byte in &mut mac {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, bssid: &str, open: bool) -> Network {
        Network {
            ssid: ssid.into(),
            bssid: parse_mac(bssid),
            open,
        }
    }

    #[test]
    fn known_prefixes_match_case_insensitively() {
        assert!(has_known_prefix("LMECHN-0A1B2"));
        assert!(has_known_prefix("Inskam-1234"));
        assert!(has_known_prefix("SUNUO-T1-XXXX"));
        assert!(!has_known_prefix("HomeNetwork"));
        assert!(!has_known_prefix("LM"));
    }

    #[test]
    fn locally_administered_bit() {
        // A locally administered BSSID like the LMECHN unit's, and a vendor-assigned one.
        assert!(is_locally_administered(
            &parse_mac("82:0a:0b:0c:0d:01").unwrap()
        ));
        assert!(!is_locally_administered(
            &parse_mac("e4:e1:30:00:00:02").unwrap()
        ));
    }

    #[test]
    fn candidates_are_open_and_ordered() {
        let scan = [
            net("HomeNetwork", "e4:e1:30:00:00:02", false),
            net("Cafe Guest", "e4:e1:30:00:00:01", true), // open, vendor BSSID
            net("MysteryCam-01", "12:34:56:78:9a:bc", true), // open, made-up BSSID
            net("LMECHN-0A1B2", "82:0a:0b:0c:0d:01", true),
            net("LMECHN-0A1B2", "82:0a:0b:0c:0d:02", true), // second BSS, same SSID
            net("inskam-locked", "02:00:00:00:00:01", false), // camera-like but secured
            net("", "02:00:00:00:00:02", true),             // hidden
        ];
        let names: Vec<&str> = candidates(&scan).iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["LMECHN-0A1B2", "MysteryCam-01"]);
    }

    #[test]
    fn known_prefix_counts_without_bssid() {
        let scan = [Network {
            ssid: "SUEAR-1".into(),
            bssid: None,
            open: true,
        }];
        assert_eq!(candidates(&scan).len(), 1);
    }

    #[test]
    fn parses_macs() {
        assert_eq!(
            parse_mac("82-0A-0B-0C-0D-01"),
            Some([0x82, 0x0a, 0x0b, 0x0c, 0x0d, 0x01])
        );
        assert_eq!(parse_mac("82:0a:0b:0c:0d"), None);
        assert_eq!(parse_mac("82:0a:0b:0c:0d:01:00"), None);
        assert_eq!(parse_mac("zz:0a:0b:0c:0d:01"), None);
    }
}
