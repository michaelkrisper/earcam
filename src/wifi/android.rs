//! Android: the camera as an app-scoped secondary network, through JNI.
//!
//! `ConnectivityManager.requestNetwork` with a `WifiNetworkSpecifier` makes Android show
//! its network picker and join the chosen network for this app only; mobile data stays the
//! default route for everything else, and [`Route`] binds EarCam's sockets to the camera.
//!
//! The specifier asks for open networks with a locally administered BSSID (see
//! [`crate::discovery`]), which covers every brand in one request; an SSID pattern could
//! only name one. Exactly one request is issued and kept: re-requesting would cancel the
//! picker out from under the user.
//!
//! Java's `NetworkCallback` cannot be subclassed from JNI, so the request is made with a
//! plain one, which is enough to keep it alive, and the granted network is found by
//! polling: a WiFi network without internet that answers GetDeviceInfo.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use jni::JNIEnv;
use jni::objects::{GlobalRef, JObject, JObjectArray, JValue};

use crate::android::{call, java_vm, jni_error, new_object, static_call, system_service};
use crate::protocol::DeviceInfo;
use crate::stream::{self, Camera, Route};

/// Generous: the user has to read and confirm the system picker.
const PICKER_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(500);

// android.net.NetworkCapabilities
const TRANSPORT_WIFI: i32 = 1;
const NET_CAPABILITY_INTERNET: i32 = 12;
/// The locally administered bit, as a BSSID base address and mask.
const LOCALLY_ADMINISTERED: &str = "02:00:00:00:00:00";

pub struct Link {
    /// The NetworkCallback that keeps the camera network request alive.
    request: Mutex<Option<GlobalRef>>,
}

impl Default for Link {
    fn default() -> Self {
        Self::new()
    }
}

impl Link {
    pub fn new() -> Self {
        Link {
            request: Mutex::new(None),
        }
    }

    /// Finds the camera: on the current network if the phone was put on the camera's WiFi
    /// by hand, otherwise through the system picker.
    pub fn connect(&self, running: &AtomicBool) -> Result<Route, String> {
        let camera = Camera::default();
        if let Some(info) = stream::probe(&camera, &Route::default(), Duration::from_millis(1500)) {
            found(&info, "the current network");
            return Ok(Route::default());
        }

        let vm = java_vm()?;
        let mut env = vm.attach_current_thread().map_err(jni_error)?;
        let cm = connectivity_manager(&mut env)?;
        let callback = request_camera_network(&mut env, &cm)?;
        *self.request.lock().unwrap() = Some(callback);
        log::info!("requested the camera network, waiting for the system picker");

        let deadline = Instant::now() + PICKER_TIMEOUT;
        while Instant::now() < deadline {
            if !running.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            for network in candidate_networks(&mut env, &cm)? {
                let route = Route {
                    network: Some(network),
                };
                if let Some(info) = stream::probe(&camera, &route, Duration::from_millis(1500)) {
                    found(&info, "the requested network");
                    return Ok(route);
                }
            }
            thread::sleep(POLL);
        }
        Err("no camera network (picker declined, or no camera in range)".into())
    }

    /// Releases the network request, which lets Android drop the camera network.
    pub fn restore(&self) {
        let Some(callback) = self.request.lock().unwrap().take() else {
            return;
        };
        let result = java_vm().and_then(|vm| {
            let mut env = vm.attach_current_thread().map_err(jni_error)?;
            let cm = connectivity_manager(&mut env)?;
            call(
                &mut env,
                &cm,
                "unregisterNetworkCallback",
                "(Landroid/net/ConnectivityManager$NetworkCallback;)V",
                &[JValue::Object(callback.as_obj())],
            )
            .map(drop)
        });
        if let Err(e) = result {
            log::warn!("releasing the camera network: {e}");
        }
    }
}

fn found(info: &DeviceInfo, on: &str) {
    log::info!("camera {} on {on}: {info}", info.ssid);
}

fn connectivity_manager<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, String> {
    system_service(env, "connectivity")
}

/// `requestNetwork` for an open WiFi network with a locally administered BSSID and no
/// internet capability; returns the callback that keeps the request alive.
fn request_camera_network(env: &mut JNIEnv, cm: &JObject) -> Result<GlobalRef, String> {
    let mac_text = env.new_string(LOCALLY_ADMINISTERED).map_err(jni_error)?;
    let mac = static_call(
        env,
        "android/net/MacAddress",
        "fromString",
        "(Ljava/lang/String;)Landroid/net/MacAddress;",
        &[JValue::Object(&mac_text)],
    )?
    .l()
    .map_err(jni_error)?;

    // No passphrase: the cameras are open, and setting one would exclude them.
    let spec = new_object(
        env,
        "android/net/wifi/WifiNetworkSpecifier$Builder",
        "()V",
        &[],
    )?;
    call(
        env,
        &spec,
        "setBssidPattern",
        "(Landroid/net/MacAddress;Landroid/net/MacAddress;)Landroid/net/wifi/WifiNetworkSpecifier$Builder;",
        &[JValue::Object(&mac), JValue::Object(&mac)],
    )?;
    let spec = call(
        env,
        &spec,
        "build",
        "()Landroid/net/wifi/WifiNetworkSpecifier;",
        &[],
    )?
    .l()
    .map_err(jni_error)?;

    let builder = "Landroid/net/NetworkRequest$Builder;";
    let request = new_object(env, "android/net/NetworkRequest$Builder", "()V", &[])?;
    call(
        env,
        &request,
        "addTransportType",
        &format!("(I){builder}"),
        &[JValue::Int(TRANSPORT_WIFI)],
    )?;
    call(
        env,
        &request,
        "removeCapability",
        &format!("(I){builder}"),
        &[JValue::Int(NET_CAPABILITY_INTERNET)],
    )?;
    call(
        env,
        &request,
        "setNetworkSpecifier",
        &format!("(Landroid/net/NetworkSpecifier;){builder}"),
        &[JValue::Object(&spec)],
    )?;
    let request = call(
        env,
        &request,
        "build",
        "()Landroid/net/NetworkRequest;",
        &[],
    )?
    .l()
    .map_err(jni_error)?;

    let callback = new_object(
        env,
        "android/net/ConnectivityManager$NetworkCallback",
        "()V",
        &[],
    )?;
    call(
        env,
        cm,
        "requestNetwork",
        "(Landroid/net/NetworkRequest;Landroid/net/ConnectivityManager$NetworkCallback;)V",
        &[JValue::Object(&request), JValue::Object(&callback)],
    )?;
    env.new_global_ref(callback).map_err(jni_error)
}

/// Handles of the WiFi networks without internet that this app can see: where the
/// requested camera network shows up once the user has picked it.
fn candidate_networks(env: &mut JNIEnv, cm: &JObject) -> Result<Vec<u64>, String> {
    // A fresh local frame per poll, or the local references would pile up on this
    // long-lived attached thread.
    env.push_local_frame(64).map_err(jni_error)?;
    let result = wifi_networks_without_internet(env, cm);
    unsafe { env.pop_local_frame(&JObject::null()) }.map_err(jni_error)?;
    result
}

fn wifi_networks_without_internet(env: &mut JNIEnv, cm: &JObject) -> Result<Vec<u64>, String> {
    let networks = call(env, cm, "getAllNetworks", "()[Landroid/net/Network;", &[])?
        .l()
        .map_err(jni_error)?;
    let networks = JObjectArray::from(networks);
    let count = env.get_array_length(&networks).map_err(jni_error)?;
    let mut handles = Vec::new();
    for i in 0..count {
        let network = env
            .get_object_array_element(&networks, i)
            .map_err(jni_error)?;
        let caps = call(
            env,
            cm,
            "getNetworkCapabilities",
            "(Landroid/net/Network;)Landroid/net/NetworkCapabilities;",
            &[JValue::Object(&network)],
        )?
        .l()
        .map_err(jni_error)?;
        if caps.is_null() {
            continue;
        }
        let wifi = call(
            env,
            &caps,
            "hasTransport",
            "(I)Z",
            &[JValue::Int(TRANSPORT_WIFI)],
        )?
        .z()
        .map_err(jni_error)?;
        let internet = call(
            env,
            &caps,
            "hasCapability",
            "(I)Z",
            &[JValue::Int(NET_CAPABILITY_INTERNET)],
        )?
        .z()
        .map_err(jni_error)?;
        if wifi && !internet {
            let handle = call(env, &network, "getNetworkHandle", "()J", &[])?
                .j()
                .map_err(jni_error)?;
            handles.push(handle as u64);
        }
    }
    Ok(handles)
}
