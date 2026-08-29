# i4season WiFi otoscope protocol

Reconstructed from the stock LMECHN app (`com.i4season.bkCamera_lmechn`, v1.0.012) and
its `libWifiCamera.so`. The same stack ships under many brands — Suear, inskam, Bebird,
LEIPUT and others — because they share the i4season OEM firmware.

Prior art: [SeanPesce/Suear-Web-Viewer](https://github.com/SeanPesce/Suear-Web-Viewer),
which documented the message layout for the older `libWifiCamera.so` clients.

## Transport

The camera is an access point. It does **not** support station mode: `libWifiCamera.so`
exports commands for resolution, exposure, LED, angle and firmware update, but nothing
that sets an SSID or passphrase. Its address is fixed.

| | |
|---|---|
| Camera IP | `192.168.1.1` |
| SSID | `LMECHN-<hex>` (other brands use `SUEAR-`, `inskam-`, …) |
| Passphrase | `12345678` (factory default, per the stock app's `Constant.java`) |
| Command port | UDP 10005 |
| Stream port | UDP 10006 |

## Command messages

12-byte header, little-endian throughout.

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `0xffeeffee` (on the wire: `ee ff ee ff`) |
| 4 | 2 | message id, incremented per request/response pair |
| 6 | 2 | message type |
| 8 | 1 | always `1` in requests |
| 9 | 1 | error code (`0` = success) |
| 10 | 2 | length of the payload that follows |

Known types: `0x01` GetDeviceInfo, `0x02` GetLicense, `0x03` SetLicense, `0x04` OpenVideo,
`0x06` UpdateFirmware, `0x0a` SetLed, `0x0c` CameraCommand, `0x0d` GetCameraConfig,
`0x0e` SetCameraConfig.

## Starting the stream

Send OpenVideo to **port 10006** from an unconnected UDP socket:

```
ee ff ee ff 00 00 04 00 01 00 00 00
```

The camera then pushes frames back to that same socket. Re-send it if frames stall.

## Stream chunks

JPEG frames are split across UDP datagrams. A single datagram may carry several chunks
back to back, each a 16-byte header followed by up to 1456 payload bytes.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | unknown, always `1` |
| 1 | 1 | chunk index (8-bit, wraps) |
| 2 | 1 | frame index (8-bit, wraps) |
| 3 | 1 | `1` on the frame's last chunk |
| 4 | 1 | total chunks — **zero until the last chunk** |
| 5 | 1 | unknown |
| 6 | 6 | orientation: three signed 16-bit axes |
| 12 | 2 | frame width |
| 14 | 2 | frame height |

Reassembly: every chunk but the last carries a full 1456-byte payload, so the first
chunk seen establishes the stride. Offsets are relative to that first chunk index, with
`+256` added when the 8-bit counter has wrapped. A frame is complete once the total is
known and the accumulated size exceeds `stride * (total - 1)`.

### Resolution

`0x0d`/`0x0e` get and set a `{bFrameIndex, wWidth, wHeight}` config, but the stock app
only ever calls the getter — it logs the result and never changes it. The camera appears
to stream at its native resolution, so EarCam does not touch this either. The exact
payload layout is unverified.

### Orientation

The three axes drive the image rotation that keeps the view upright while the probe is
turned. The mapping from axes to roll angle is **not verified against hardware** — see
`Orientation.angleDegrees`, which currently assumes `atan2(y, x)`. Long-press the image
in the app to overlay the raw values for calibration.
