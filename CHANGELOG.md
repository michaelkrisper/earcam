# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The Android app and the desktop client were
separate projects up to 0.1; from 0.2 they are one codebase.

## [0.3.0] - 2026-10-20

### Added
- The picture is mirrored left to right by default. Double-tap toggles the mirror on
  Android, **m** on the desktop. A long press now resets the zoom.
- The LED is switched on at full brightness after connecting (the LMECHN unit has it
  on already).
- The gravity vector is taken only from chunks that flag it.
- `EARCAM_HOME_WIFI` names the network the desktop returns to on quit when none was
  active at start.

### Changed
- The image no longer jumps when the probe points almost straight up or down: beyond
  85° tilt the roll is held, and the deadband widens near the vertical.
- docs/protocol.md rewritten: chunk header flags, announcements, LED and the full table
  of message types.
- README: features, gestures, privacy and disclaimer; CONTRIBUTING.md and issue
  templates added.

### Removed
- The built-in fallback network on the desktop; use `EARCAM_HOME_WIFI` instead.

## [0.2.1] - 2026-09-27

### Changed
- Android: JNI code ported to jni 0.22 (closure-based thread attachment with a fresh
  local reference frame per call, signatures checked at compile time). The APK now
  carries a single jni version. Verified on a Pixel 4a 5G.
- Gradle 9.8.0; the minimal `gradlew` launcher stays.

## [0.2.0] - 2026-09-27

### Added
- One Rust codebase for Android, Linux and Windows. On Android, the platform's
  NativeActivity loads the Rust library directly; there is no Java or Kotlin left.
- Cameras of any i4season brand are found, not only `LMECHN-*`. Open networks with a
  known brand prefix or a locally administered BSSID are candidates, and a network
  counts as a camera once it answers GetDeviceInfo. The chip, firmware and battery level
  are logged.
- Windows support: WiFi switching through the native WLAN API with a temporary profile,
  a console Ctrl handler for a clean quit, and a desktop GL 3.3 fallback. Built and
  tested in CI, not yet run against a camera.
- The desktop restores whichever network was active at start.
- Verified on hardware: Linux desktop, and the Android app on a Pixel 4a 5G (17 fps,
  camera found through the system picker by its BSSID).
- End-to-end tests against a simulated camera on localhost, and unit tests for
  discovery, device info and touch gestures.
- CI builds and tests all three platforms. Tagging `v*` publishes one release with the
  APK, the Linux archive, the Windows executable and checksums.

### Changed
- Android keeps the screen on with a wake lock (`WAKE_LOCK` permission) instead of a
  window flag.
- The desktop retries reconnecting the previous WiFi, whose first association after
  leaving the camera can fail.
- The video port is registered from the video socket itself, so frames pass stateful
  firewalls as replies.
- Android: the APK carries a 1.1 MB native library instead of ~56 KB of Kotlin, the
  price of sharing one codebase. JPEG decoding moved from `BitmapFactory` to zune-jpeg.
- Android: the network request matches open networks with a locally administered BSSID
  instead of the `LMECHN` name prefix, so other brands appear in the system picker too.

## [0.1.0] - 2026-09-27

### Added
- Desktop client for Linux (Rust): joins `LMECHN-*` through NetworkManager, streams at
  the camera's 17 fps, renders fullscreen with the Android app's deblocking and rotation
  shaders, and reconnects the home network on quit.
- Android app (Kotlin): OpenGL rendering with luma and chroma deblocking, pinch zoom,
  rotation from the probe's gravity sensor, app-scoped camera network so mobile data
  stays up, and the port registration (type `0x16`) that actually starts this
  firmware's stream.
