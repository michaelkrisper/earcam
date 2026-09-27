//! Windows backend, through the native WLAN API (wlanapi.dll). Unlike `netsh wlan`, it
//! can trigger a scan and does not have localized output to parse.
//!
//! The camera is joined with a temporary profile, which Windows discards on disconnect.
//! The network to return to is reconnected by its saved profile name.

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::NetworkManagement::WiFi::{
    DOT11_SSID, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_ATTRIBUTES,
    WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST, WlanCloseHandle, WlanConnect,
    WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanOpenHandle,
    WlanQueryInterface, WlanScan, dot11_BSS_type_infrastructure, wlan_connection_mode_profile,
    wlan_connection_mode_temporary_profile, wlan_interface_state_connected,
    wlan_intf_opcode_current_connection,
};
use windows_sys::core::GUID;

use super::{Connection, open_profile_xml};

/// How long to wait for an association to complete.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Backend {
    handle: HANDLE,
    interface: GUID,
}

impl Drop for Backend {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.handle, null()) };
    }
}

fn check(call: &str, status: u32) -> Result<(), String> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "{call} failed: {}",
            std::io::Error::from_raw_os_error(status as i32)
        ))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(s: &[u16]) -> String {
    let len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..len])
}

fn ssid_string(ssid: &DOT11_SSID) -> String {
    let len = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
    String::from_utf8_lossy(&ssid.ucSSID[..len]).into_owned()
}

impl Backend {
    /// Opens a WLAN client session on the first wireless interface.
    pub fn open() -> Result<Backend, String> {
        unsafe {
            let mut version = 0;
            let mut handle: HANDLE = null_mut();
            // Client version 2: Windows Vista and later.
            check(
                "WlanOpenHandle",
                WlanOpenHandle(2, null(), &mut version, &mut handle),
            )?;
            // Owns the handle from here on, so every early return closes it.
            let mut backend = Backend {
                handle,
                interface: std::mem::zeroed(),
            };

            let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
            check(
                "WlanEnumInterfaces",
                WlanEnumInterfaces(handle, null(), &mut list),
            )?;
            let first =
                ((*list).dwNumberOfItems > 0).then(|| (*list).InterfaceInfo[0].InterfaceGuid);
            WlanFreeMemory(list as *const c_void);
            backend.interface = first.ok_or("no WiFi interface")?;
            Ok(backend)
        }
    }

    pub fn current(&self) -> Result<Option<Connection>, String> {
        unsafe {
            let mut size = 0;
            let mut data: *mut c_void = null_mut();
            let status = WlanQueryInterface(
                self.handle,
                &self.interface,
                wlan_intf_opcode_current_connection,
                null(),
                &mut size,
                &mut data,
                null_mut(),
            );
            // ERROR_INVALID_STATE: not connected.
            if status != ERROR_SUCCESS {
                return Ok(None);
            }
            let attrs = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
            let connection =
                (attrs.isState == wlan_interface_state_connected).then(|| Connection {
                    profile: from_wide(&attrs.strProfileName),
                    ssid: ssid_string(&attrs.wlanAssociationAttributes.dot11Ssid),
                });
            WlanFreeMemory(data);
            Ok(connection)
        }
    }

    /// Visible SSIDs. Triggers a fresh scan, whose results show up on the next call;
    /// the caller polls every couple of seconds anyway.
    pub fn scan(&self) -> Result<Vec<String>, String> {
        unsafe {
            // Refused while a scan is running, which is fine.
            WlanScan(self.handle, &self.interface, null(), null(), null());
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
            check(
                "WlanGetAvailableNetworkList",
                WlanGetAvailableNetworkList(self.handle, &self.interface, 0, null(), &mut list),
            )?;
            let networks = std::slice::from_raw_parts(
                (*list).Network.as_ptr(),
                (*list).dwNumberOfItems as usize,
            );
            let ssids = networks.iter().map(|n| ssid_string(&n.dot11Ssid)).collect();
            WlanFreeMemory(list as *const c_void);
            Ok(ssids)
        }
    }

    pub fn join(&self, ssid: &str) -> Result<(), String> {
        let bytes = ssid.as_bytes();
        let mut dot11 = DOT11_SSID {
            uSSIDLength: bytes.len().min(32) as u32,
            ucSSID: [0; 32],
        };
        dot11.ucSSID[..dot11.uSSIDLength as usize]
            .copy_from_slice(&bytes[..dot11.uSSIDLength as usize]);
        let xml = wide(&open_profile_xml(ssid));
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_temporary_profile,
            strProfile: xml.as_ptr(),
            pDot11Ssid: &mut dot11,
            pDesiredBssidList: null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        check("WlanConnect", unsafe {
            WlanConnect(self.handle, &self.interface, &params, null())
        })?;
        self.wait_for(|c| c.ssid == ssid)
    }

    /// Nothing to do: the temporary profile goes away when the next connection replaces it.
    pub fn leave_camera(&self) {}

    pub fn activate(&self, profile: &str) -> Result<(), String> {
        let name = wide(profile);
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: name.as_ptr(),
            pDot11Ssid: null_mut(),
            pDesiredBssidList: null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        check("WlanConnect", unsafe {
            WlanConnect(self.handle, &self.interface, &params, null())
        })?;
        self.wait_for(|c| c.profile == profile)
    }

    /// WlanConnect only starts the association; this polls until it has happened.
    fn wait_for(&self, done: impl Fn(&Connection) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        while Instant::now() < deadline {
            if self.current()?.is_some_and(|c| done(&c)) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(250));
        }
        Err("timed out waiting for the WiFi connection".into())
    }
}
