# EarCam Desktop

Linux client for LMECHN WiFi ear cameras (and the other i4season-OEM otoscopes), a port
of the [Android app](https://github.com/michaelkrisper/earcam). Rust, fullscreen, the live
image rotated upright inside a circle on black. The protocol is documented there in
`docs/protocol.md`.

```
cargo build --release
./target/release/earcam-desktop
```

On start it waits up to a minute for an `LMECHN-*` network, joins it through
NetworkManager (`nmcli`) and starts the stream. `q`, `Escape`, closing the window or
Ctrl+C quit and reconnect to the home network. Both names are hardcoded in `src/wifi.rs`.
The laptop's card has one WiFi interface, so it is offline while the camera is connected.
The camera profile is in memory only and never autoconnects.

Status and a once-a-second line with fps, decode time and the raw gravity vector go to
stderr.

Rendering is GLES 3.0 with the Android app's shaders unchanged: luma and chroma
deblocking on the JPEG block grid, then rotation and circular mask. It redraws only when
a camera frame arrives.
