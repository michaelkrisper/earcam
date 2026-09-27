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
//! polling: a WiFi network without internet that answers GetDeviceInfo. Each poll runs in
//! its own JNI frame, so the long-lived thread does not accumulate local references.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use jni::errors::Result as JniResult;
use jni::objects::{Global, JObject, JObjectArray};
use jni::{Env, JValue, jni_sig, jni_str};

use crate::android::{call, system_service, with_env};
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
    request: Mutex<Option<Global<JObject<'static>>>>,
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

        let callback = with_env("requesting the camera network", request_camera_network)?;
        *self.request.lock().unwrap() = Some(callback);
        log::info!("requested the camera network, waiting for the system picker");

        let deadline = Instant::now() + PICKER_TIMEOUT;
        while Instant::now() < deadline {
            if !running.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            for network in with_env("listing networks", wifi_networks_without_internet)? {
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
        let result = with_env("releasing the camera network", |env| {
            let cm = system_service(env, "connectivity")?;
            call(
                env,
                &cm,
                jni_str!("unregisterNetworkCallback"),
                jni_sig!("(Landroid/net/ConnectivityManager$NetworkCallback;)V"),
                &[JValue::Object(callback.as_obj())],
            )
            .map(drop)
        });
        if let Err(e) = result {
            log::warn!("{e}");
        }
    }
}

fn found(info: &DeviceInfo, on: &str) {
    log::info!("camera {} on {on}: {info}", info.ssid);
}

/// `requestNetwork` for an open WiFi network with a locally administered BSSID and no
/// internet capability; returns the callback that keeps the request alive.
fn request_camera_network(env: &mut Env) -> JniResult<Global<JObject<'static>>> {
    let cm = system_service(env, "connectivity")?;
    let mac_text = env.new_string(LOCALLY_ADMINISTERED)?;
    let mac = env
        .call_static_method(
            jni_str!("android/net/MacAddress"),
            jni_str!("fromString"),
            jni_sig!("(Ljava/lang/String;)Landroid/net/MacAddress;"),
            &[JValue::Object(&mac_text)],
        )?
        .l()?;

    // No passphrase: the cameras are open, and setting one would exclude them.
    let spec = env.new_object(
        jni_str!("android/net/wifi/WifiNetworkSpecifier$Builder"),
        jni_sig!("()V"),
        &[],
    )?;
    call(
        env,
        &spec,
        jni_str!("setBssidPattern"),
        jni_sig!(
            "(Landroid/net/MacAddress;Landroid/net/MacAddress;)Landroid/net/wifi/WifiNetworkSpecifier$Builder;"
        ),
        &[JValue::Object(&mac), JValue::Object(&mac)],
    )?;
    let spec = call(
        env,
        &spec,
        jni_str!("build"),
        jni_sig!("()Landroid/net/wifi/WifiNetworkSpecifier;"),
        &[],
    )?
    .l()?;

    let request = env.new_object(
        jni_str!("android/net/NetworkRequest$Builder"),
        jni_sig!("()V"),
        &[],
    )?;
    call(
        env,
        &request,
        jni_str!("addTransportType"),
        jni_sig!("(I)Landroid/net/NetworkRequest$Builder;"),
        &[JValue::Int(TRANSPORT_WIFI)],
    )?;
    call(
        env,
        &request,
        jni_str!("removeCapability"),
        jni_sig!("(I)Landroid/net/NetworkRequest$Builder;"),
        &[JValue::Int(NET_CAPABILITY_INTERNET)],
    )?;
    call(
        env,
        &request,
        jni_str!("setNetworkSpecifier"),
        jni_sig!("(Landroid/net/NetworkSpecifier;)Landroid/net/NetworkRequest$Builder;"),
        &[JValue::Object(&spec)],
    )?;
    let request = call(
        env,
        &request,
        jni_str!("build"),
        jni_sig!("()Landroid/net/NetworkRequest;"),
        &[],
    )?
    .l()?;

    let callback = env.new_object(
        jni_str!("android/net/ConnectivityManager$NetworkCallback"),
        jni_sig!("()V"),
        &[],
    )?;
    call(
        env,
        &cm,
        jni_str!("requestNetwork"),
        jni_sig!(
            "(Landroid/net/NetworkRequest;Landroid/net/ConnectivityManager$NetworkCallback;)V"
        ),
        &[JValue::Object(&request), JValue::Object(&callback)],
    )?;
    env.new_global_ref(callback)
}

/// Handles of the WiFi networks without internet that this app can see: where the
/// requested camera network shows up once the user has picked it.
fn wifi_networks_without_internet(env: &mut Env) -> JniResult<Vec<u64>> {
    let cm = system_service(env, "connectivity")?;
    let networks = call(
        env,
        &cm,
        jni_str!("getAllNetworks"),
        jni_sig!("()[Landroid/net/Network;"),
        &[],
    )?
    .l()?;
    let networks = env.cast_local::<JObjectArray<JObject>>(networks)?;
    let mut handles = Vec::new();
    for i in 0..networks.len(env)? {
        let network = networks.get_element(env, i)?;
        let caps = call(
            env,
            &cm,
            jni_str!("getNetworkCapabilities"),
            jni_sig!("(Landroid/net/Network;)Landroid/net/NetworkCapabilities;"),
            &[JValue::Object(&network)],
        )?
        .l()?;
        if caps.is_null() {
            continue;
        }
        let has = |env: &mut Env, method, value| {
            call(env, &caps, method, jni_sig!("(I)Z"), &[JValue::Int(value)])?.z()
        };
        let wifi = has(env, jni_str!("hasTransport"), TRANSPORT_WIFI)?;
        let internet = has(env, jni_str!("hasCapability"), NET_CAPABILITY_INTERNET)?;
        if wifi && !internet {
            let handle = call(
                env,
                &network,
                jni_str!("getNetworkHandle"),
                jni_sig!("()J"),
                &[],
            )?
            .j()?;
            handles.push(handle as u64);
        }
    }
    Ok(handles)
}
