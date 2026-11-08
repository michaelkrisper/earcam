# CLAUDE.md

EarCam is a minimal viewer for i4season-firmware WiFi ear cameras (otoscopes). One Rust
library (edition 2024) serves Android, Linux and Windows. It has exactly one window:
the live image, kept upright, inside a circle on black. There is a mirror toggle and,
on Android, touch zoom. There are no settings and no other UI. Keep it that way.

## Commands

```
cargo test                                                     # unit tests + tests/fake_camera.rs
cargo fmt --check
cargo clippy --all-targets -- -D warnings                      # Linux
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo ndk -t arm64-v8a clippy --all-targets -- -D warnings     # needs ANDROID_NDK_HOME
cd android && ./gradlew assembleRelease                        # APK; runs cargo ndk itself
```

- `tests/fake_camera.rs` runs a fake camera over local UDP and is the only automated
  end-to-end test. Real-camera tests are done by the user.
- `ci.yml` checks format, then runs clippy, tests and builds for Linux, Windows and
  Android on every push and PR. Windows binaries come from CI.
- The Gradle configuration cache is on (`android/gradle.properties`): build logic must
  be configuration-cache safe. There is no Kotlin or Java (`android.builtInKotlin=false`).

## Architecture (`src/`)

- `protocol.rs`, `assembler.rs`, `stream.rs`: wire format, frame reassembly, sockets,
  JPEG decode (zune-jpeg) and the triple-buffer `Exchange`. `stream::probe`
  (GetDeviceInfo) identifies a camera.
- `orientation.rs`: roll from the probe's gravity vector, easing, hold near vertical.
- `discovery.rs`: candidate networks are open networks with a known brand prefix or a
  locally administered BSSID. A GetDeviceInfo reply is always the confirmation.
- `wifi/`: `desktop.rs` holds the scan, join and probe loop, and restores the previous
  network or `$EARCAM_HOME_WIFI` on exit. It sits on `linux.rs` (nmcli, in-memory
  profile) or `windows.rs` (wlanapi, temporary profile). `android.rs` uses JNI
  `requestNetwork` with a BSSID-pattern `WifiNetworkSpecifier` and no passphrase.
- `app.rs`: the winit/glutin app shared by all platforms. `render.rs`: GLES 3.0 with a
  desktop GL 3.3 fallback. Two passes: deblocking, then rotation, zoom and circular
  mask.
- `session.rs`: connect and stream on a worker thread; `stop()` undoes the connection.
  `signals.rs` turns SIGINT/SIGTERM/SIGHUP or Windows console events into a clean quit,
  so the WiFi is always restored.

## Rules

- **Never run the desktop app yourself.** It joins the camera's WiFi, so a laptop with
  one WiFi card goes offline, and this session with it. Ask the user to run it and paste
  the log.
- Read `docs/protocol.md` before touching `protocol.rs`, `assembler.rs` or
  `orientation.rs`. It records what works on this firmware and what doesn't.
  - Streaming starts with message type `0x16` to port 10006, not `OpenVideo`.
  - The gravity vector is three 10-bit sign-magnitude fields, not three `int16`.
- Render only once per camera frame (about 17 fps). Don't reintroduce display-rate
  rendering; touch gestures are the only exception.
- Android:
  - Never call mutating `AndroidApp` methods such as `set_window_flags`, on any thread.
    The event loop holds the lock and deadlocks, which ends in an ANR. The screen stays
    on through a `PowerManager` wake lock via JNI instead.
  - Issue exactly one network request; requesting again cancels the system picker.
  - Sockets are bound to the camera network (`android_setsocknetwork`), so mobile data
    stays the default route.
  - `android_main` ends with `process::exit`, because winit's event loop can't be
    created twice per process.
- Image pipeline choices were A/B-tested on hardware:
  - Deblocking stays.
  - Denoise and sharpening were rejected.
  - No ML super-resolution: invented detail is unacceptable in an otoscope image.
  - The firmware has no image-quality, exposure or frame-rate controls. Don't add any.
- Android `targetSdk` stays at 36 until Android 17's local-network protection has been
  checked against the UDP traffic to the camera.
- The APK is signed with the debug key on purpose, so CI artifacts install directly.
- This repo is public: no personal data, home network names, device identifiers or
  local paths in commits, docs or logs.

## Workflow

- Every change starts with a GitHub issue. Branch `<issue>-<slug>` from `main`, open a
  PR with `Closes #<issue>`. Never push to `main` directly.
- PRs auto-merge (squash) once CI and the "Linked ticket" check pass; drafts are not merged.
- Versions exist only as git tags `vX.Y.Z`. Every merge to `main` tags the next patch.
  `build.rs` reads the version from `git describe` (`EARCAM_VERSION`); the version in
  `Cargo.toml` is only a fallback. Never bump it.
- Minor/major releases: Actions -> Release. `release.yml` then builds all platforms
  via `ci.yml` and uploads the binaries with `SHA256SUMS.txt` to the GitHub release.
- Details: CONTRIBUTING.md.
