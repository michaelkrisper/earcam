# EarCam

[![CI](https://github.com/michaelkrisper/earcam/actions/workflows/ci.yml/badge.svg)](https://github.com/michaelkrisper/earcam/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/michaelkrisper/earcam)](https://github.com/michaelkrisper/earcam/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Android%20%7C%20Linux%20%7C%20Windows-informational)

A small, fast, open-source viewer for **WiFi ear cameras** (otoscopes) running the
i4season firmware, sold under brands such as LMECHN, Suear, inskam, Bebird and LEIPUT.
One Rust codebase runs on **Android, Linux and Windows**.

EarCam shows the live image inside a circle on black, kept upright by the probe's own
gravity sensor. It finds and joins the camera's WiFi by itself, and on Android your
mobile data keeps working while the camera is connected. There are no menus, no
settings and no accounts.

## Features

- **Upright image:** the picture follows the probe's gravity sensor, eased and steady
  against hand tremor, and holds still when the probe points straight up or down.
- **Mirrored view** by default, which feels natural when looking into your own ear;
  toggle it with a double-tap (Android) or **m** (desktop).
- **Pinch to zoom** and drag to pan on Android.
- **Automatic connection:** scans for the camera, joins it, and on the desktop
  reconnects your previous network on exit.
- **Clean picture:** a GPU deblocking pass removes the JPEG block grid without
  inventing detail.
- **Lightweight:** ~1.1 MB APK, no Java or Kotlin, no network access beyond the camera.

## Download

| Platform | File | Status |
|---|---|---|
| Android 14+ (arm64) | `earcam-android-arm64.apk` | stable, tested on hardware |
| Linux x86_64 (NetworkManager) | `earcam-linux-x86_64.tar.gz` | tested on hardware |
| Windows 10/11 x86_64 | `earcam-windows-x86_64.exe` | built and tested in CI, not yet tested with a camera |

Get the file for your platform from the
[latest release](https://github.com/michaelkrisper/earcam/releases/latest). Each release
includes `SHA256SUMS.txt`.

- **Android:** install the APK and allow installing from unknown sources. It is signed
  with a debug key so that CI builds install as well.
- **Linux:** `tar xzf earcam-linux-x86_64.tar.gz && ./earcam`. Requires NetworkManager
  (`nmcli`), which desktop distributions ship by default. No root needed.
- **Windows:** run `earcam-windows-x86_64.exe`. If Windows Firewall asks, allow it on
  public networks, because Windows treats an unknown open WiFi as public.

## Usage

Switch the camera on, then start EarCam.

**Android:** confirm the camera in the system's network picker. If the phone is already
on the camera's WiFi, EarCam uses it directly.

| Gesture | Action |
|---|---|
| Pinch | Zoom |
| Drag | Pan (when zoomed) |
| Double-tap | Toggle mirror |
| Long press | Reset zoom |
| Back | Quit |

**Linux / Windows:** EarCam searches for up to a minute, joins the camera and runs
fullscreen. Press **m** to toggle the mirror and **q** or **Esc** to quit. On quit it
reconnects the network that was active before. If none was, set `EARCAM_HOME_WIFI` to
the network it should return to:

```
EARCAM_HOME_WIFI="My WiFi" ./earcam
```

Most laptops have a single WiFi card, so **a laptop is offline while the camera is
connected**. A wired connection or phone tethering keeps it online. The camera
connection is never saved, so no device joins the camera on its own later.

Status messages and a once-a-second summary go to the terminal (desktop) or to logcat
(`adb logcat -s EarCam`):

```
camera LMECHN-59F77: BK7231U-XRH-FBPRO (YPC), firmware HKV48B, battery 97%
17.2 fps, decode 3.36 ms, 480x480, 17588 bytes, x=-62 y=3 z=-27
```

## Supported cameras

EarCam works with cameras that run the i4season firmware, whatever the brand on the
box. Such a camera

- is an **open WiFi access point** at **192.168.1.1**,
- answers a device query on UDP port 10005,
- and streams JPEG frames over UDP once a client registers a port.

EarCam finds candidates in a WiFi scan: open networks whose name starts with a known
brand prefix (`LMECHN`, `SUNUO`, `SUEAR`, `inskam`), or whose BSSID is *locally
administered*, which these cameras use instead of a manufacturer-assigned MAC. That
second rule covers brands EarCam has never seen. A candidate counts as a camera only
once it answers the device query.

| Camera | Reports | Status |
|---|---|---|
| LMECHN N30 Pro | `BK7231U-XRH-FBPRO (YPC)`, firmware `HKV48B` | verified |
| Other i4season-based otoscopes (Suear, inskam, Bebird, LEIPUT, SUNUO, …) | | expected to work, reports welcome |

If you try EarCam with another camera, please
[open an issue](https://github.com/michaelkrisper/earcam/issues/new/choose) with the
`camera ...` status line, whether it works or not.

## How it works

```
 camera 192.168.1.1                                  EarCam (one Rust library)
 ──────────────────                                  ─────────────────────────
 UDP 10005 <- device query  (identify)   ────────── wifi/ + discovery.rs
 UDP 10006 <- register video port        ────────── stream.rs
 UDP 10006 -> JPEG chunks + gravity vector ───────>   reassemble (assembler.rs)
                                                      decode JPEG (zune-jpeg)
                                                            │ triple buffer, no copies
                                                            ▼
                                                     app.rs + render.rs (OpenGL ES 3)
                                                      1. deblock 8x8 luma / 16x16 chroma
                                                      2. rotate upright, mirror, zoom, circle
```

| Module | Role |
|---|---|
| `protocol.rs` | Wire format: 12-byte command header, 16-byte chunk header, device info |
| `assembler.rs` | Frames from out-of-order chunks: global wrapping chunk counter, only the last chunk carries the total |
| `stream.rs` | Sockets, port registration (re-sent when frames stall), decode, triple buffer |
| `orientation.rs` | Roll = `atan2(z, y)` of the probe's accelerometer, eased, with a tilt-dependent deadband |
| `render.rs` | Deblocking where the JPEG's block edges are artifacts, then rotation, mirror, zoom and mask |
| `discovery.rs`, `wifi/` | Finding and joining the camera: `nmcli` (Linux), WLAN API (Windows), `ConnectivityManager` via JNI (Android) |
| `app.rs`, `gestures.rs` | The window, redrawn once per camera frame, plus touch gestures |

On Android the platform's `NativeActivity` loads `libearcam.so` directly. The WiFi
request goes through JNI, and the camera sockets are bound to the camera network with
`android_setsocknetwork`, so mobile data stays the default route.

Image processing was compared side by side on real hardware. Deblocking stays;
denoising and sharpening were rejected, because the camera's JPEGs are compressed to
~0.6 bit/pixel and anything sharper than bilinear sampling turns the block grid into a
mosaic. There is deliberately no ML upscaling: invented detail has no place in an
otoscope image.

The protocol is documented in [docs/protocol.md](docs/protocol.md).

## Building from source

Requirements: Rust (stable). For Android also JDK 21+ (CI uses 25), the Android SDK with
NDK `30.0.16248370`, and `cargo install cargo-ndk`.

```
cargo run --release                          # desktop app
cargo test                                   # unit tests and end-to-end stream tests
cd android && ./gradlew assembleRelease      # APK in android/app/build/outputs/apk/release/
```

`android/local.properties` must point to the SDK (`sdk.dir=/path/to/Android/Sdk`). The
Gradle build runs `cargo ndk` itself.

Checking all platforms from Linux:

```
cargo clippy --all-targets -- -D warnings                                  # Linux
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings   # Windows
cargo ndk -t arm64-v8a clippy --all-targets -- -D warnings                 # Android
```

### Tests

- **Unit tests** cover the wire format, device info parsing, frame reassembly (reorder,
  duplicates, counter wrap-around, stale frames), orientation, candidate networks,
  touch gestures and `nmcli` output parsing.
- **`tests/fake_camera.rs`** simulates a camera on localhost: it answers the device
  query, waits for the port registration and streams a real 480x480 JPEG in
  firmware-sized chunks, with variants that reorder chunks, duplicate them and ignore
  the first registration.

Only the WiFi switch, the window and the camera itself need real hardware.

### CI and releases

Every push and pull request runs formatting, clippy and tests on Linux and Windows,
clippy for Android, and builds all three packages ([ci.yml](.github/workflows/ci.yml)).
To release, set the version in `Cargo.toml`, add a section to
[CHANGELOG.md](CHANGELOG.md) and push a matching tag (`git tag v0.3.0 && git push origin
v0.3.0`). [release.yml](.github/workflows/release.yml) builds every platform, checks
the tag against `Cargo.toml` and publishes the release with checksums and the changelog
section as release notes.

## Troubleshooting

| Symptom | Check |
|---|---|
| `no camera found` | Is the camera on and charged? It needs a few seconds to boot. Is its WiFi open (no password)? |
| A network is tried but `does not answer like a camera` | It is not an i4season-firmware camera, or its firmware differs. Please open an issue with the log. |
| Connected, but no picture | The `cmd 0x16 ... err=0` line should appear. Older firmware that needs a different start command is not supported yet. |
| Picture rotated wrongly | Open an issue with the `x/y/z` values from the status line while holding the probe upright. |
| Desktop not back on its WiFi after quitting | EarCam was killed hard (e.g. `kill -9`). Reconnect by hand; the camera profile is never saved. |

## Privacy

EarCam communicates only with the camera. It contacts no server, has no analytics and
no accounts, and does not store images or any other data.

## Contributing

Bug reports, camera compatibility reports and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## Disclaimer

EarCam is an independent project. It is not affiliated with, endorsed by or sponsored
by i4season, LMECHN or any other camera manufacturer or brand; all product names and
trademarks belong to their respective owners and are used only to describe
compatibility.

EarCam is **not a medical device** and is not intended for diagnosis or treatment. If
you have ear problems, see a doctor. Insert the probe carefully and only as far as is
comfortable. The software is provided "as is", without warranty of any kind; see the
[license](LICENSE).

## License

[MIT](LICENSE) © 2026 Michael Krisper
