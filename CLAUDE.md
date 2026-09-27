# CLAUDE.md

Desktop port (Rust) of the EarCam Android app in `~/earcam` (github.com/michaelkrisper/earcam). There, `docs/protocol.md` is the authoritative protocol write-up and `CLAUDE.md` lists the protocol gotchas; keep `protocol.rs`, `assembler.rs`, `orientation.rs` and the shaders in `render.rs` in sync with the Android app.

## Commands

```
cargo test                                               # unit + tests/fake_camera.rs
cargo clippy --all-targets -- -D warnings                # CI runs this on Linux and Windows
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings   # Windows type-check from Linux
cargo fmt --check
```

There is no mingw linker on the dev laptop, so Windows binaries come only from CI (`.github/workflows/ci.yml`, tags `v*` publish a release).

## Hardware constraints

- The dev laptop's BCM4360 (`wl`) supports one managed interface, and the home LAN "Katze" is also `192.168.1.0/24` with gateway `.1`. **Running the app takes the laptop, and this session, offline.** Never run it yourself; the user runs it and pastes the log.
- Verified on hardware (Linux, 2026-09-27): 17 fps, ~3.4 ms decode, rotation correct. Windows has never run against a camera.
- The camera must see the port registration (type `0x16`, sent from the video socket) before it streams.

## Layout

Library (`src/lib.rs`) with everything testable; `src/main.rs` is only window + GL context + wiring. `wifi/linux.rs` (nmcli, in-memory profile `earcam`) and `wifi/windows.rs` (wlanapi, temporary profile) implement the same `Backend` methods used by `wifi/mod.rs`. `signals.rs` turns SIGINT/SIGTERM/SIGHUP or Windows console events into a clean quit so the WiFi is always restored.
