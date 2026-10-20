# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

EarCam: a minimal client for i4season-firmware WiFi ear cameras (otoscopes), sold as LMECHN, Suear, inskam, Bebird, LEIPUT and others. **One Rust library for Android, Linux and Windows.** It has one window: the live image rotated upright inside a circle on black. There are no settings and no UI beyond that (plus touch zoom on Android). Keep it that way.

The protocol was reverse-engineered from the stock app (`com.i4season.bkCamera_lmechn` v1.0.012, decompiled under `~/tools/re/out/lmechn`) and its `libWifiCamera.so`. `docs/protocol.md` is the authoritative write-up. Read it before touching `protocol.rs`, `assembler.rs` or `orientation.rs`; it records which message types work on this firmware and which are dead ends.

## Commands

```
cargo test                                                     # unit + tests/fake_camera.rs
cargo clippy --all-targets -- -D warnings                      # Linux
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo ndk -t arm64-v8a clippy --all-targets -- -D warnings     # needs ANDROID_NDK_HOME
cargo fmt --check
cd android && ./gradlew assembleRelease                        # APK, runs cargo ndk itself
```

For `cargo ndk` by hand: `export ANDROID_NDK_HOME=~/Android/Sdk/ndk/30.0.16248370`. `android/local.properties` (gitignored) holds `sdk.dir=/home/michi/Android/Sdk`. Gradle 9.7.1 / AGP 9.4.1 run on JDK 25 via `~/.gradle/gradle.properties` (`org.gradle.java.home`). The configuration cache is on, so build logic must be configuration-cache safe (no script references captured in task actions). `android.builtInKotlin=false`: there is no Kotlin or Java in the project.

There is no mingw linker on the dev laptop, so Windows binaries come only from CI. `ci.yml` builds and tests every platform on each push. Pushing a tag `vX.Y.Z` that matches `Cargo.toml` runs `release.yml`, which calls `ci.yml` and publishes one release with all binaries, checksums and the CHANGELOG section. The APK is signed with the debug key on purpose, so CI artifacts are installable.

## Architecture

- `protocol.rs`, `assembler.rs`, `stream.rs`: wire format, frame reassembly, sockets + decode (zune-jpeg) + triple buffer `Exchange`. `stream::probe` (GetDeviceInfo) identifies a camera.
- `discovery.rs`: candidate networks are open networks with a known brand prefix or a **locally administered BSSID**. The camera firmware makes up its MAC (`82:77:8a:01:58:74`, verified), while the home router `e4:e1:30:…` is vendor-assigned. Confirmation is always a GetDeviceInfo reply.
- `wifi/`: `desktop.rs` holds the scan → join → probe loop and restores the previous network (fallback `Katze`), on top of `linux.rs` (nmcli, in-memory profile `earcam`) or `windows.rs` (wlanapi, temporary profile, BSS list for BSSID/privacy). `android.rs` uses JNI: `requestNetwork` with a `WifiNetworkSpecifier` BSSID pattern `02:00:00:00:00:00/02:00:00:00:00:00` and **no passphrase** (the AP is open; a passphrase excludes it). It uses a plain `NetworkCallback`, because JNI cannot subclass it, and finds the granted network by polling `getAllNetworks()` (WiFi without INTERNET that answers GetDeviceInfo). `stream::Route` binds sockets with `android_setsocknetwork`, so mobile data stays the default route. Issue exactly one request: re-requesting cancels the system picker.
- `app.rs`: the winit/glutin app shared by all platforms. It renders **only once per camera frame** (17 fps); do not reintroduce display-rate rendering. The only exception is touch gestures. On Android the frame rate is declared with `ANativeWindow_setFrameRate(17, FIXED_SOURCE)`, and the GL state is dropped and recreated on suspend/resume. **Never call mutating `AndroidApp` methods such as `set_window_flags`, on any thread:** they take a write lock that the event loop holds as a reader while it runs. That deadlocks the loop and ends in an ANR (seen on hardware). The screen is kept on with a `PowerManager` wake lock via JNI instead (`src/android.rs`). `android_main` ends with `process::exit`, because winit's event loop cannot be created twice per process.
- `render.rs`: GLES 3.0 (desktop GL 3.3 fallback, same source with a different `#version`). There are two passes: deblocking (luma 8x8, chroma 16x16, `ALPHA`/`BETA`), then rotation, zoom and circular mask with bilinear sampling. **A/B-tested on hardware:** deblocking kept; bilateral/temporal denoise and Catmull-Rom/CAS sharpening rejected; no ML super-resolution (invented detail is unacceptable in an otoscope image).
- `session.rs`: connect + stream on a worker thread, `stop()` undoes the connection. `signals.rs` (desktop) turns SIGINT/SIGTERM/SIGHUP or Windows console events into a clean quit, so the WiFi is always restored.

## Protocol gotchas

- **Starting the stream is not `OpenVideo` (`0x04`).** This firmware acks it with `err=0` and sends nothing. What works: register the client's UDP port with **type `0x16` sent to camera port 10006**, payload a little-endian `uint16`. The header length says `0` while the 2 bytes follow (the stock client's quirk). EarCam sends it from the video socket itself and re-sends if frames stall.
- Ports: commands UDP 10005, stream UDP 10006, the camera pushes a 29-byte type `0x09` announcement to client port 10007 once a second (ignored; useful as a sign of life). Camera IP is fixed at `192.168.1.1`. GetDeviceInfo replies 12 + 128 bytes.
- The gravity vector at chunk offset 6 is a `uint32` of **three 10-bit sign-magnitude fields** (z = bits 0-9, y = 10-19, x = 20-29). Reading it as three `int16` yields garbage.
- Rotation is verified on hardware (2026-09-27): roll `atan2(z, y)`, image drawn at `-90deg - roll`. Beyond 85deg tilt (probe pointing up/down) the roll is held, and the deadband widens near vertical, as in the stock library (not yet hardware-tested).
- Resolution get/set (`0x0d`/`0x0e`) exists, but the N30 Pro lacks capability bit `0x40`: 480x480 is fixed. No message sets JPEG quality or frame rate.

## Hardware and testing constraints

- The only way to test end to end is real hardware: the LMECHN N30 Pro (reports `BK7231U-XRH-FBPRO`, chip unverified, possibly a Taixin TXW816; firmware HKV48B) and the user's phone. There are no instrumented tests and no emulator path.
- The dev laptop's BCM4360 (`wl`) supports one managed interface, and the home LAN "Katze" is also `192.168.1.0/24` with gateway `.1`. **Running the desktop app takes the laptop, and this session, offline.** Never run it yourself; the user runs it and pastes the log.
- Verified on hardware (2026-09-27): Linux desktop (17 fps, ~3.4 ms decode, rotation correct) and the Rust Android app on a Pixel 4a 5G, Android 14 (17 fps, 3-6 ms decode, camera found through the BSSID-pattern picker). Windows has not run against a camera yet. The Pixel has no SIM, so "mobile data stays up" is unverified with the Rust app.
- Debugging on the phone: `./gradlew assembleDebug` gives a debuggable APK, so `adb shell run-as com.krisper.earcam cat /proc/<pid>/task/<tid>/wchan` shows where a thread is blocked. A healthy `android_main` sits in `do_epoll_wait`.
- Android `targetSdk` stays at 36 until Android 17's local-network protection has been checked against the UDP traffic to `192.168.1.1`.
