# EarCam

[![CI](https://github.com/michaelkrisper/earcam/actions/workflows/ci.yml/badge.svg)](https://github.com/michaelkrisper/earcam/actions/workflows/ci.yml)
[![Release](https://github.com/michaelkrisper/earcam/actions/workflows/release.yml/badge.svg)](https://github.com/michaelkrisper/earcam/releases)

A small, fast viewer for **WiFi ear cameras** (otoscopes) built on the i4season OEM
firmware, sold as LMECHN, Suear, inskam, Bebird, LEIPUT and more. One Rust codebase
runs on **Android, Linux and Windows**.

It shows the live image, rotated upright by the probe's own gravity sensor, inside a
circle on black. It has no menus and no settings. It finds and joins the camera's WiFi
itself, and on Android your mobile data keeps working while the camera is connected.

| Platform | Package | Size | Status |
|---|---|---|---|
| Android 14+ (arm64) | `earcam-android-arm64.apk` | ~1.1 MB | stable, verified on hardware |
| Linux (x86_64, NetworkManager) | `earcam-linux-x86_64.tar.gz` | ~2.5 MB binary | verified on hardware |
| Windows 10/11 (x86_64) | `earcam-windows-x86_64.exe` | ~0.4 MB | built and tested in CI, **not yet run against a camera** |

## Supported cameras

EarCam works with cameras that run the i4season firmware. On the wire that means:

- the camera is an **open WiFi access point** at **192.168.1.1**,
- it answers **GetDeviceInfo** on UDP port 10005,
- and it streams JPEG frames over UDP once a client registers a port.

The brand on the box does not matter; the firmware is what counts. EarCam recognises a
camera in two steps:

1. **Candidate networks** from a WiFi scan: open networks whose name starts with a
   known brand prefix (`LMECHN`, `SUNUO`, `SUEAR`, `inskam`), or whose BSSID is
   *locally administered*. The Beken BK7231U firmware makes up its MAC address instead
   of using one from a manufacturer range, e.g. `82:77:8a:…`. That rule covers brands
   whose network names EarCam has never seen.
2. **Confirmation:** after joining, the camera must answer GetDeviceInfo. The answer
   names the chip. Networks that do not answer are left again.

| Camera | Chip / firmware | Status |
|---|---|---|
| LMECHN N30 Pro (`LMECHN-59F77`) | Beken BK7231U (`BK7231U-XRH-FBPRO`), firmware `HKV48B` | verified |
| Other i4season-based otoscopes (Suear, inskam, Bebird, LEIPUT, SUNUO, …) | same firmware family | expected to work, reports welcome |

The status line prints what the camera reports, e.g.
`camera LMECHN-59F77: BK7231U-XRH-FBPRO (YPC), firmware HKV48B, battery 97%`. Please
include that line when reporting a camera that works or does not.

Older Suear-era units that only start streaming on the `OpenVideo` command are not
supported (see [docs/protocol.md](docs/protocol.md)).

## Install

Download the file for your platform from the latest
[release](https://github.com/michaelkrisper/earcam/releases). Every
[CI run](https://github.com/michaelkrisper/earcam/actions/workflows/ci.yml) also keeps
the binaries of its commit as artifacts.

- **Android:** install the APK. It is signed with a debug key, so allow installing
  from unknown sources.
- **Linux:** `tar xzf earcam-linux-x86_64.tar.gz && ./earcam`. It needs
  NetworkManager (`nmcli`), which desktop distributions ship by default, and no root.
- **Windows:** run `earcam-windows-x86_64.exe`. If Windows Firewall asks, allow it on
  public networks, because Windows treats an unknown open WiFi as public.

## Usage

Switch the camera on, then start EarCam.

- **Android:** confirm the camera in the system's network picker. Pinch to zoom, drag
  to pan, double-tap to reset, and use Back to quit. If the phone is already on the
  camera's WiFi, EarCam uses it directly.
- **Linux / Windows:** EarCam searches for up to a minute, joins the camera, and runs
  fullscreen. Press **q** or **Esc** to quit. On quit it reconnects the network that
  was active before (or `Katze` if there was none). Closing the window and Ctrl+C work
  too.

Most laptops have a single WiFi card, so **a laptop is offline while the camera is
connected**. A wired connection or phone tethering keeps it online. The camera
connection is never saved, so no system joins the camera on its own later.

Status and a once-a-second line go to the terminal (desktop) or to logcat
(`adb logcat -s EarCam`):

```
17.2 fps, decode 3.36 ms, 480x480, 17588 bytes, x=-62 y=3 z=-27
```

## How it works

```
 camera 192.168.1.1                                  EarCam (one Rust library)
 ──────────────────                                  ─────────────────────────
 UDP 10005 <- GetDeviceInfo  (identify)  ────────── wifi/ + discovery.rs
 UDP 10006 <- register video port (0x16) ────────── stream.rs
 UDP 10006 -> JPEG chunks + gravity vector ───────>   reassemble (assembler.rs)
                                                      decode JPEG (zune-jpeg)
                                                            │ triple buffer, no copies
                                                            ▼
                                                     app.rs + render.rs (OpenGL ES 3)
                                                      1. deblock 8x8 luma / 16x16 chroma
                                                      2. rotate upright, zoom, circle
```

| Module | Role |
|---|---|
| `protocol.rs` | Wire format: 12-byte command header (magic `0xffeeffee`), 16-byte chunk header, device info |
| `assembler.rs` | Frames from out-of-order chunks: global wrapping chunk counter, only the last chunk carries the total |
| `stream.rs` | Sockets, port registration (re-sent when frames stall), decode, triple buffer |
| `orientation.rs` | Roll = `atan2(z, y)` of the probe's accelerometer, eased, with a deadband against hand tremor |
| `render.rs` | H.264-style deblocking where the JPEG's block edges are artifacts, then rotation, zoom and mask |
| `discovery.rs`, `wifi/` | Finding and joining the camera: `nmcli` (Linux), WLAN API (Windows), `ConnectivityManager` via JNI (Android) |
| `app.rs`, `gestures.rs` | The window, redrawn once per camera frame, plus touch gestures |

The platform layer is thin. On Android, the platform's `NativeActivity` loads
`libearcam.so` directly, with no Java or Kotlin. The WiFi request is made through JNI,
and the camera sockets are bound to the camera network with `android_setsocknetwork`,
so mobile data stays the default route.

Image processing was compared on hardware. Deblocking stays; denoising and sharpening
were rejected. The camera's JPEGs are ~0.6 bit/pixel, and anything sharper than bilinear
sampling turns the block grid into a mosaic. There is no ML upscaling either: invented
detail is unacceptable in an otoscope image.

The full reverse-engineered protocol is in [docs/protocol.md](docs/protocol.md).

## Building

Requirements: Rust stable. For Android you also need JDK 21+ (CI uses 25), the Android
SDK with NDK `30.0.16248370`, and `cargo install cargo-ndk`.

```
cargo run --release                          # desktop app
cargo test                                   # unit tests + end-to-end stream tests
cd android && ./gradlew assembleRelease      # APK -> android/app/build/outputs/apk/release/
```

`android/local.properties` must point to the SDK (`sdk.dir=/path/to/Android/Sdk`). The
Gradle build runs `cargo ndk` itself.

Checking other platforms from Linux:

```
cargo clippy --all-targets -- -D warnings                                  # Linux
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings   # Windows
cargo ndk -t arm64-v8a clippy --all-targets -- -D warnings                 # Android
```

### Tests

- **Unit tests** cover the wire format, device info parsing, frame reassembly (reorder,
  duplicates, counter wrap-around, stale frames), rotation, candidate networks, touch
  gestures, and `nmcli` output parsing.
- **`tests/fake_camera.rs`** simulates a camera on localhost. It answers GetDeviceInfo,
  waits for the port registration, and streams a real 480x480 4:2:0 JPEG in
  firmware-sized chunks. Variants reorder chunks, duplicate them, and ignore the first
  registration.

Only the WiFi switch, the window and the device itself need real hardware.

### CI and releases

Every push and pull request runs formatting, clippy and tests on Linux and Windows,
clippy for Android, and builds all three packages
([ci.yml](.github/workflows/ci.yml)). To release, set the version in `Cargo.toml`, add
a section to [CHANGELOG.md](CHANGELOG.md), and push a matching tag:

```
git tag v0.2.0 && git push origin v0.2.0
```

[release.yml](.github/workflows/release.yml) builds every platform, checks that the tag
matches `Cargo.toml`, and publishes one release with the APK, the Linux archive, the
Windows executable, `SHA256SUMS.txt` and the changelog section as release notes.

## Troubleshooting

| Symptom | Check |
|---|---|
| `no camera found` | Is the camera on and charged? It needs a few seconds to boot. Is its WiFi open (no password)? |
| A network is tried but `does not answer like a camera` | It is not an i4season camera, or its firmware differs; please open an issue with the log. |
| Connected, but no picture | The `cmd 0x16 ... err=0` line should appear. Older firmware that needs `OpenVideo` is not supported. |
| Picture rotated wrongly | Open an issue with the `x/y/z` values from the status line while holding the probe upright. |
| Desktop not back on its WiFi after quitting | EarCam was killed hard (e.g. `kill -9`). Reconnect by hand; the camera profile is never saved. |

## License

[MIT](LICENSE)
