# EarCam

A minimal Android client for LMECHN WiFi ear cameras (and the other i4season-based
otoscopes). One screen: the live image, rotated upright on black, and a left/right ear
toggle. Nothing else.

## Why

The stock app is a Chinese-language file manager wrapped around a video view, and using
it means switching the phone onto the camera's WiFi — which takes the phone offline.
EarCam requests the camera network through `WifiNetworkSpecifier` and binds only its own
socket to it, so mobile data keeps working normally.

## Design

- **Kotlin + Jetpack Compose**, but deliberately without Material 3 or icon packs — the
  only chrome is a two-segment pill, which is cheaper to draw than to depend on.
- **Three dependencies**: Compose UI, Compose foundation, activity-compose. R8 and
  resource shrinking on.
- **No cropping.** The camera's sensor image is rectangular; the round look comes from
  the optics' vignette. The full frame is shown, scaled so rotation never clips a corner.
- **Quality first in the hot path**: ARGB_8888 decoding, `FilterQuality.High`, a 4 MB
  UDP receive buffer (frame bursts otherwise drop packets and visibly stutter), and a
  conflated flow so latency stays flat when rendering falls behind.

Minimum Android 14 (API 34).

## Install

Grab the APK from the latest [build artifact](../../actions) or
[release](../../releases), then sideload it. Power on the camera and start the app —
it finds the `LMECHN-*` access point on its own.

## Calibration

The camera reports its orientation with every frame, but the axis-to-angle mapping is not
documented and has not been verified against hardware. If the image rotates the wrong way
or by the wrong amount, long-press it to overlay the raw sensor values, then adjust
`Orientation.angleDegrees`.

## Protocol

See [docs/protocol.md](docs/protocol.md).

## Build

```
./gradlew assembleRelease
```
