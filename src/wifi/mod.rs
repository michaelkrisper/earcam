//! Gets the device onto the camera's network, and off it again.
//!
//! - **Linux** (NetworkManager) and **Windows** (WLAN API): a laptop's WiFi card joins one
//!   network at a time, so EarCam switches it to the camera and back to the previous
//!   network on quit. Candidates come from a scan ([`crate::discovery`]); each is joined and
//!   asked for its device info, and the first that answers is the camera.
//! - **Android**: the camera is requested as an app-scoped secondary network through
//!   `ConnectivityManager`, so mobile data stays up; see `android.rs`.
//!
//! The camera connection is never saved, so no system joins the camera on its own later.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{Link, finish, toast};

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
pub use desktop::Link;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
compile_error!("EarCam supports Android, Linux (NetworkManager) and Windows");

/// Returned to on quit (desktop) when no other network was connected at start.
pub const HOME_FALLBACK: &str = "Katze";

/// The network an interface is connected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The saved profile, which is what the desktop [`Link`] reconnects by.
    pub profile: String,
    pub ssid: String,
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
