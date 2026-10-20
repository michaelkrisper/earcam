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
| Other | TCP 5007 is open (full scan of 1–10010) but sends no banner; purpose unknown. `libWifiCamera.so` never references it. |
| Client address | DHCP, first lease `192.168.1.10` |

The LMECHN-59F77 unit (N30 Pro) reports itself as `BK7231U-XRH-FBPRO` (see
[Device info](#device-info)), but that is only a product string. A
[teardown of an otoscope with the same string](https://www.elektroda.com/rtvforum/topic4129331.html)
found a Taixin **TXW816** (C-SKY CK803, hgSDK) and a HI708 sensor that identifies as
480x480, not a Beken chip. The N30 Pro has not been opened, so its chip is unverified. Its
USB-C port is charge-only: plugged into a PC, nothing enumerates.

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
and `0x14`–`0x16` (sent to 10006) on top of that table, plus `0x07`/`0x08` and
`0x17`–`0x19` for firmware update and `0x1a` for setting the clock.

What the library does with the rest, read from its code and **not exercised on the N30
Pro** unless noted:

| Type | Port | Payload | Purpose |
|---|---|---|---|
| `0x0a` | 10005 | 3 bytes: LED number (low nibble) and 0 = get / 1 = set (high nibble); state 0 off, 1 on, 2 blink, 3 breathe, 4 button; brightness 0–100 | LED. The stock app sends `11 01 64` (LED 1 on at 100 %) 500 ms after connecting; EarCam does the same before registering the stream, if capability bit `0x01` is set. |
| `0x0b` | 10005 | `uint16` count, then that many 8-bit chunk indices | Retransmission request, only for stream packet format 2 (see [Stream chunks](#stream-chunks)). The N30 Pro sends format 1, which has no retransmission. |
| `0x0c` | 10005 | `item` (capability bit), type (low nibble, 0 get / 1 set) and action (high nibble), `int32` value | Generic command; the stock app only uses it for the work mode, and only if the camera advertises one. |
| `0x0f` | 10005 | `uint32` port | Tells the camera where to push file notifications; the stock app only does this for one other product. |
| `0x10` / `0x11` | 10005 | 12 bytes: `int32` auto exposure, `int32` unknown, `int32` absolute exposure | Get / set exposure, UVC style. Never called by the stock app. |
| `0x12` / `0x13` | 10005 | 40 bytes, ten `int32`: brightness, contrast, hue, saturation, sharpness, gamma, auto white balance, white balance temperature, backlight compensation, gain | Get / set image parameters, UVC style. Never called by the stock app; whether this firmware honours them is unknown. |
| `0x14` | 10006 | `uint16` port | Registers the client's [announcement](#device-announcements) port. The library re-sends it after 15 s without an announcement. |
| `0x15` | 10006 | 1 byte | Low-power mode, only sent if bit 2 of the device info flags is set. That bit is clear on the N30 Pro, hence the error 2 it returns. |
| `0x1b` | 10005 | 1 byte | Record status sync, only if bit 6 of the device info flags is set. |

`0x03` (SetLicense) writes a 176-byte block with display angle, offsets and a checksum
into the camera's persistent license area. Do not send it.

## Device info

Type `0x01` with no payload, e.g. `ee ff ee ff 01 00 01 00 01 00 00 00`. The reply payload
is exactly 128 bytes (the client rejects anything else):

| Offset | Size | Field | LMECHN-59F77 |
|---|---|---|---|
| 0x00 | 1 | unknown | `05` |
| 0x01 | 32 | vendor | `YPC` (followed by leftover bytes: `ota ok`, `sys_cfgs.license = `) |
| 0x21 | 32 | product | `BK7231U-XRH-FBPRO` |
| 0x41 | 16 | firmware version | `HKV48B` |
| 0x51 | 32 | SSID | `LMECHN-59F77` |
| 0x71 | 6 | MAC | `82:77:8a:01:58:74` |
| 0x77 | 2 | power: battery % = `>> 9`, bit 8 = charging | 97 %, not charging |
| 0x79 | 1 | capability bits (`CAMCAP_*` in the stock app): `0x01` light, `0x02` G-sensor, `0x04` motor, `0x08` AD effect, `0x10` mode, `0x20` disable battery, `0x40` resolution switching, `0x80` G-sensor MJPEG | `0x03` |
| 0x7a | 1 | work mode 1 | |
| 0x7b | 1 | work mode 2 (signed) | |
| 0x7c | 1 | flags: bit 2 = low-power mode, bit 4 = wants time set, bit 6 = record status sync | `0x00` |

If bit 4 of the flags is set, the stock client follows up with type `0x1a` (year `uint16`,
then month, day, hour, minute, second as bytes). A read-only query must stop at `0x01`.

## Firmware update

Reconstructed from `cameraWifiupdateFirmware` in `libWifiCamera.so`; **never exercised**.
The stock app declares `WifiCamera.updateFirmware(String)` but never calls it, ships no
image and has no download URL (`Constant.UPDATA_url`, `simicloud.com/.../version.xml`, is
unused and returns 404). No image for these cameras is publicly available.

All on UDP 10005, a fresh socket per message, 100 ms receive timeout, up to 30 retries.
The header length field stays `0` throughout.

1. Start: type `0x06`, no payload. The ack must carry a 2-byte payload: the chunk size
   (`uint16`) the camera wants. The client caps it at 4084.
2. Data: type `0x07`, header plus raw file bytes, one chunk per message. No offset,
   index or checksum; order comes only from the incrementing message id. Every chunk
   must be acked with error `0`.
3. End: type `0x08`, no payload.

The MCU path is identical with types `0x17`/`0x18`/`0x19`. The client sends the file as
is: no header parsing, size, MD5 or version check. Whatever validation exists happens on
the camera; Beken OTA images are normally RBL-wrapped and CRC-checked by the bootloader,
but that is unverified here — and if the chip is a Taixin rather than a Beken (see
[Transport](#transport)), the image format is unknown altogether. A rejected image is the
best case; an accepted broken one can likely only be recovered through debug pads inside
the sealed housing.

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

The library's regular path (`proOpenVideo`) is a different message: type `0x04` sent to
**port 10006** (not 10005), length 2, payload the video port as `uint16` — 4 bytes with
a second (audio) port appended if a device flag is set. It checks once a second and
re-sends it as soon as no stream packet arrived; after 3 s without data it reconnects from
scratch. This variant has not been tried on the N30 Pro; `0x16` is verified and enough.

`0x14` has the same shape as `0x16` but registers the announcement port, and `0x15` is a
low-power switch (see [Command messages](#command-messages)).

## Device announcements

Once associated, the camera sends a 29-byte type `0x09` message to **client** port 10007
every second, from an ephemeral port of its own:

```
ee ff ee ff 00 00 09 00 00 00 11 00 02 0c 01 00 00 00 00 58 …
```

EarCam ignores these; they are listed here because they are the only unsolicited traffic
that arrives before the stream is registered, which makes them a useful sign of life.

The library decodes the payload (after the 12-byte header) as follows; **unverified**, and
the sample above would decode to a battery value of 6, so the battery field may be a
level rather than a percentage:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | subtype: 1 status, 2 status with buttons, 3 discovery |
| 1 | 2 | bit 0 charging, bits 1–7 battery, bit 8 sensor ok, bits 9–15 low-power-off |
| 3 | 1 | work mode |
| 4 | 1 | motor level (signed) |
| 5 | 2 | subtype 2: four 4-bit rolling counters for the photo, video, zoom-down and zoom-up buttons |
| 7 | 2 | subtype 2: last two MAC bytes |
| 9 | 1 | subtype 2: frozen (low nibble), mirror (high nibble) |
| 16 | 1 | subtype 2: camera status (low nibble), picture change id (high nibble) |

If header byte 8 is non-zero, the camera expects the header echoed back.

## Stream chunks

JPEG frames are split across UDP datagrams, each a 16-byte header followed by the
payload. This firmware sends one chunk per datagram with a 1280-byte payload (1296-byte
datagrams). The LMECHN N30 Pro streams 480x480 at a steady ~17 fps, ~16-19 KB per JPEG
(15-16 chunks); decoding one takes ~4 ms on a Pixel 4a 5G. The reference client allows several chunks packed back to back in
one datagram, with payloads up to 1456 bytes, so EarCam parses that case too.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | packet format, always `1` here (see below) |
| 1 | 1 | chunk index (8-bit, wraps) |
| 2 | 1 | frame index (8-bit, wraps) |
| 3 | 1 | `1` on the frame's last chunk |
| 4 | 1 | total chunks — zero until the last chunk on older firmware, already set on the first chunk here |
| 5 | 1 | flags: bit 0 = gravity present, bits 1–2 unknown, bit 3 = offset 6 holds temperature/humidity instead |
| 6 | 4 | gravity vector, packed (see below) |
| 10 | 2 | last two bytes of the camera's MAC (`58 74` here) |
| 12 | 2 | frame width |
| 14 | 2 | frame height |

Read from `libWifiCamera.so`, not observed: packet format `1` is this 16-byte header,
`6` a 28-byte one (payload from offset `0x1c`), `5` audio PCM and `2` a variant whose
lost chunks the client re-requests with type `0x0b`. The library takes the gravity vector
only from chunks with flags `& 0x09 == 0x01`, reconnects if the MAC bytes do not match
the device info, and drops a frame unless it starts with `FF D8` and has `FF D9` in its
last 16 bytes. Its reassembly drops a frame on any gap or reordering; EarCam's is more
tolerant. EarCam only logs the flags (`chunk flags 0x..`) until a hardware log shows
what the N30 Pro sends in byte 5.

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
capability field of the device info. The N30 Pro reports `0x03`, so the stock library does
not even query the list: 480x480 is fixed. The protocol has no message for JPEG quality
or frame rate either; both are up to the firmware.

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

When the probe points nearly straight up or down, gravity runs along `x` and the roll is
undefined. The stock library computes a tilt `atan(|x| / hypot(y, z))` and freezes the
angle beyond 85°; below that it holds the angle until it moves by more than a threshold
that grows with the tilt, from 0.4° when level to 11° near vertical. EarCam does the same
with its own numbers: no update beyond 85°, and a deadband of at least 3° that widens as
`1 rad / hypot(y, z)`, the angle one sensor unit is worth. Its smoothing (the stock
library jumps) and other client-side angle options (`cameraChangeAngleDiff`,
`cameraDelShake`) never reach the camera.
