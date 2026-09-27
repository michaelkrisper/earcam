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
| Security | **Open.** Verified by scan: `LMECHN-59F77 … [ESS]`, no RSN/WPA flags. The stock app's `Constant.java` carries a `12345678` passphrase, but this unit does not use it. |
| Command port | UDP 10005 |
| Stream port | UDP 10006 |
| Notify port | UDP 10007 (on the *client*) |

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

The `libWifiCamera.so` shipped with the current app adds `0x10`–`0x13` (sent to 10005)
and `0x14`–`0x16` (sent to 10006) on top of that table.

## Starting the stream

The OpenVideo of the older Suear-era clients does **not** work on this firmware. It is
acknowledged with `err=0` and then nothing happens — no frames, on any port. The 22785
those clients bind is not a protocol constant either: it never appears in
`libWifiCamera.so`, it was just the port that one client happened to get.

What the stock client actually does (`openVideoForceApi`, and the socket setup a few
hundred bytes earlier in the same object):

1. Bind a UDP socket, letting the OS pick the port.
2. Read that port back with `getsockname()`.
3. Send **type `0x16`** to **port 10006** with the port as a little-endian `uint16`:

   ```
   ee ff ee ff <id:2> 16 00 01 00 00 00 <port:2>
   ```

   Note the length field says `0` although two payload bytes follow — the stock client
   sends 14 bytes with a zero length, and the camera does not mind.

Frames start arriving on the registered socket immediately, from source port 10006.
Re-send the message if they stall.

`0x14` has the same shape and registers the audio port; `0x15` carries a single `1`/`0`
byte and looks like a stream on/off switch, but this unit answers it with error 2 and
streams fine without it.

## Device announcements

Once associated, the camera sends a 29-byte type `0x09` message to **client** port 10007
every second, from an ephemeral port of its own:

```
ee ff ee ff 00 00 09 00 00 00 11 00 02 0c 01 00 00 00 00 58 …
```

EarCam ignores these; they are listed here because they are the only unsolicited traffic
that arrives before the stream is registered, which makes them a useful sign of life.

## Stream chunks

JPEG frames are split across UDP datagrams, each a 16-byte header followed by the
payload. This firmware sends one chunk per datagram with a 1280-byte payload (1296-byte
datagrams). The LMECHN N30 Pro streams 480x480 at a steady ~17 fps, ~16-19 KB per JPEG
(15-16 chunks); decoding one takes ~4 ms on a Pixel 4a 5G. The reference client allows several chunks packed back to back in
one datagram, with payloads up to 1456 bytes, so EarCam parses that case too.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | unknown, always `1` |
| 1 | 1 | chunk index (8-bit, wraps) |
| 2 | 1 | frame index (8-bit, wraps) |
| 3 | 1 | `1` on the frame's last chunk |
| 4 | 1 | total chunks — zero until the last chunk on older firmware, already set on the first chunk here |
| 5 | 1 | unknown |
| 6 | 4 | gravity vector, packed (see below) |
| 10 | 2 | unknown, `0x7458` on this unit |
| 12 | 2 | frame width |
| 14 | 2 | frame height |

Reassembly: the chunk index is a **global** 8-bit counter that keeps running across
frames (a frame of 15 chunks might be `112..126`), so a frame's first index is only known
from its last chunk: `first = last - total + 1` (mod 256). Chunks are therefore parked by
index until the last one arrives, and a frame is complete when every index in that range
is present. The earlier approach — offsets relative to the first chunk *seen*, complete
once enough bytes arrived — breaks on reordered first chunks and on duplicates.

The frame index wraps every 256 frames (~15 s at 17 fps). A frame that lost a chunk must
be dropped within a few frames: left in place, its stale chunks get merged into the next
frame with the same index 256 frames later, which then completes one chunk early, leaves
its real last chunk behind as the next stale entry, and keeps the corruption going
indefinitely (observed on hardware as an undecodable frame every ~15 s).

### Resolution

`0x0d`/`0x0e` get and set a `{bFrameIndex, wWidth, wHeight}` config, but the stock app
only ever calls the getter — it logs the result and never changes it. The camera appears
to stream at its native resolution, so EarCam does not touch this either. The exact
payload layout is unverified.

480x480 is native, not a mode to escape: the stock app never calls the setter, has no UI
for it, and when saving a photo from a 480x480 stream it upscales it to 2300x2300
(`I4seasonUsbCamera.isSavePhotoSuccful`) — the "HD" photos are interpolated. Whether a
unit could switch at all is advertised by `CAMCAP_ResolutionSwitching` (`0x40`) in the
capability field of the device info, which has not been checked on the N30 Pro.

### Orientation

Offset 6 holds a little-endian `uint32` with three 10-bit fields, each a 9-bit magnitude
with the sign in bit 9 — the layout `sensor_get_xyz` in `libWifiCamera.so` produces:

| Bits | Axis |
|---|---|
| 0–9 | z |
| 10–19 | y |
| 20–29 | x |

Reading the same bytes as three `int16` (as older write-ups do) yields garbage. The
vector is gravity, about 60 units at 1 g. Calibrated on hardware: `x` runs along the
probe and stays constant while it is rolled; `y` and `z` sweep a circle. Roll is
`atan2(z, y)`, zero in the rest position and increasing when the probe is turned
counter-clockwise as seen from behind. The sensor image comes out a quarter turn off
(bottom on the left), so EarCam draws it at `-90° - roll`.

The camera does not stabilise the image itself; the stock app rotates on the phone too.
