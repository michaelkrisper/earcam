# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Windows support: WiFi switching through the native WLAN API with a temporary profile,
  a console Ctrl handler for a clean quit, and a desktop GL 3.3 fallback where no GLES
  context is available. Built and tested in CI, not yet run against a camera.
- The network that was active at start is restored on quit; `Katze` is only the fallback.
- End-to-end test against a simulated camera on localhost.
- CI on Linux and Windows with release binaries for tags.

### Changed
- The video port is registered from the video socket itself, so the frames pass
  stateful firewalls as replies.
- Split into a library and a thin binary.

## [0.1.0] - 2026-09-27

### Added
- First release for Linux: joins `LMECHN-*` through NetworkManager, streams at the
  camera's 17 fps, renders fullscreen with the Android app's deblocking and rotation
  shaders, and reconnects the home network on quit.
