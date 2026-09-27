# EarCam Desktop

[![CI](https://github.com/michaelkrisper/earcam-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/michaelkrisper/earcam-desktop/actions/workflows/ci.yml)

A small, fast desktop client for **LMECHN WiFi ear cameras** (otoscopes). It is the
desktop port of the [EarCam Android app](https://github.com/michaelkrisper/earcam).
Other i4season-OEM units (Suear, inskam, Bebird, LEIPUT) use the same protocol but
different network names; EarCam only looks for `LMECHN-*` (`CAMERA_PREFIX` in
`src/wifi/mod.rs`).

Start it and it joins the camera's WiFi, shows the live image fullscreen, rotated upright
inside a circle on black, and reconnects your normal WiFi when you quit. It has no
settings and no UI beyond the picture.

| | |
|---|---|
| Platforms | Linux (NetworkManager, Wayland or X11) and Windows 10/11 |
| Binary | a single ~2.5 MB executable, no runtime dependencies |
| Performance | 17 fps (the camera's rate), ~3.4 ms JPEG decode per frame on a 2014 Intel laptop |

## Usage

1. Switch the camera on.
2. Run `earcam-desktop`.
3. Press **q** or **Esc** to quit.

On start EarCam waits up to a minute for a network named `LMECHN-*`, joins it, and starts
the stream. On quit it reconnects whichever network was active before, or `Katze` if
there was none. Quitting also works by closing the window or with Ctrl+C in the
terminal. The camera connection is never saved, so your system will not join the camera
on its own later.

Most laptops have a single WiFi card, so **the computer is offline while the camera is
connected**. A wired connection or phone tethering keeps you online.

Status goes to the terminal, including one line per second:

```
17.2 fps, decode 3.36 ms, 480x480, 17588 bytes, x=-62 y=3 z=-27
```

`x/y/z` is the probe's raw gravity vector, which drives the rotation.

## Install

Prebuilt binaries for Linux and Windows come from [Releases](../../releases) (tagged
versions) or from the artifacts of any [CI run](../../actions).

Build from source (Rust stable):

```
cargo build --release
./target/release/earcam-desktop
```

### Linux

Requires NetworkManager (`nmcli`), which desktop distributions ship by default. No root
is needed, because polkit lets the logged-in user manage WiFi.

### Windows

Uses the native WLAN API. There is nothing to install.

> **Not yet tested on real hardware.** The Windows build is compiled, linted and
> unit-tested in CI (including the end-to-end stream test), but it has not run against a
> camera yet. Reports are welcome.

If Windows Firewall asks on first start, allow EarCam on public networks: an unknown open
WiFi is classified as public. EarCam sends its stream registration from the socket that
receives the video, so the frames count as replies and normally need no exception.

## How it works

```
 camera (192.168.1.1)                             EarCam
 ────────────────────                             ──────
 UDP 10005 <- device info query  ───────────────  stream thread
 UDP 10006 <- register video port (type 0x16)       │ receive datagrams (4 MB buffer)
 UDP 10006 -> JPEG chunks + gravity vector  ─────>  │ reassemble frames   (assembler.rs)
                                                    │ decode JPEG         (zune-jpeg)
                                                    ▼
                                              triple buffer, no copies (stream.rs)
                                                    ▼
                                              GL thread, once per frame (render.rs)
                                                1. deblock 8x8 luma / 16x16 chroma
                                                2. rotate upright, circular mask
```

- **Protocol** (`protocol.rs`): 12-byte little-endian command header with magic
  `0xffeeffee`. This firmware ignores the `OpenVideo` command older clients send. The
  stream starts when the client registers its UDP port with type `0x16`. The full
  reverse-engineered write-up is `docs/protocol.md` in the Android repository.
- **Reassembly** (`assembler.rs`): the chunk counter is global and wraps, and only a
  frame's last chunk carries the total. Chunks are parked by index, so reordering,
  duplicates and wrap-around do not matter. Stale partial frames are dropped before the
  8-bit frame counter comes round again.
- **Rotation** (`orientation.rs`): roll is `atan2(z, y)` of the probe's accelerometer,
  eased with a small deadband so the image does not tremble with the hand.
- **Rendering** (`render.rs`): the camera's JPEGs are compressed hard (~0.6 bit/pixel).
  An H.264-style deblocking filter smooths block edges only where both sides are flat and
  the step is small. Anything sharper than bilinear sampling was tested on the Android
  app and rejected, because it turns the block grid into a mosaic. EarCam uses GLES 3.0,
  or desktop GL 3.3 where GLES is unavailable, and redraws only when a frame arrives.
- **WiFi** (`wifi/`): `nmcli` with an in-memory profile on Linux, and a temporary WLAN
  profile through `wlanapi.dll` on Windows.

## Development

```
cargo test                      # unit tests + end-to-end stream test
cargo clippy --all-targets      # lints, as in CI
cargo fmt                       # formatting, as in CI
```

`tests/fake_camera.rs` simulates the camera on localhost. It answers commands, waits
for the port registration, and streams a real 480x480 4:2:0 JPEG in firmware-sized
chunks with a wrapping chunk counter. Variants reorder chunks, duplicate them, and
ignore the first registration. The only parts that need real hardware are the WiFi
switch and the window.

To type-check the Windows code from Linux:

```
rustup target add x86_64-pc-windows-gnu
cargo clippy --target x86_64-pc-windows-gnu --all-targets
```

CI builds and tests on Linux and Windows for every push. Tags `v*` publish a release with
both binaries.

## Troubleshooting

| Symptom | Check |
|---|---|
| `no LMECHN-* network found` | Is the camera on and charged? It needs a few seconds to boot. |
| Connected, but no picture | The `cmd 0x16 ... err=0` line should appear. If it does not, the camera is not reachable at 192.168.1.1. |
| Picture rotated wrongly | Note the `x/y/z` values from the status line with the probe held upright and open an issue. |
| Not back on your WiFi after quitting | EarCam was killed hard (e.g. `kill -9`). Reconnect manually; the camera profile is never saved. |

## License

[MIT](LICENSE)
