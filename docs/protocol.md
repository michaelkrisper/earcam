# Camera protocol

This document describes the network protocol of i4season-firmware WiFi otoscopes as
EarCam implements it. The same firmware ships under several brands (LMECHN, Suear,
inskam, Bebird, LEIPUT and others). Everything marked **verified** was confirmed on an
LMECHN N30 Pro; the rest describes the protocol family and has not been exercised.

## Transport

The camera is a WiFi access point. It cannot join another network, so the client always
connects to the camera.

| | |
|---|---|
| Camera IP | `192.168.1.1` (fixed) |
| SSID | `LMECHN-<hex>`; other brands use `SUEAR-`, `inskam-`, … |
| Security | Open network (verified: no RSN/WPA flags in the scan) |
| Command port | UDP 10005 |
| Stream port | UDP 10006 |
| Announcement port | UDP 10007, on the *client* |
| Client address | DHCP, first lease `192.168.1.10` |

The access point's BSSID is *locally administered* (e.g. `82:77:8a:01:58:74`), which
EarCam uses to recognise cameras whose network name it does not know.

## Command messages

Every command and reply starts with a 12-byte little-endian header.

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `0xffeeffee` (on the wire: `ee ff ee ff`) |
| 4 | 2 | message id, incremented per request; the reply repeats it |
| 6 | 2 | message type |
| 8 | 1 | `1` in requests |
| 9 | 1 | error code (`0` = success) |
| 10 | 2 | length of the payload that follows |

Message types:

| Type | Port | Payload | Purpose |
|---|---|---|---|
| `0x01` | 10005 | none | GetDeviceInfo, see [Device info](#device-info). **Verified.** |
| `0x02` / `0x03` | 10005 | | Device license data. `0x03` writes to persistent storage; never send it. |
| `0x04` | 10006 | `uint16` video port, length 2 | OpenVideo, the alternative way to start the stream. Not verified; the same type without payload, sent to 10005, is acknowledged by the N30 Pro but starts nothing. |
| `0x06`–`0x08`, `0x17`–`0x19` | 10005 | | Firmware update. EarCam never sends these. |
| `0x09` | → 10007 | 17 bytes | Announcement from the camera, see [Announcements](#announcements). |
| `0x0a` | 10005 | 3 bytes | LED, see [LED](#led). **Verified.** |
| `0x0b` | 10005 | `uint16` count, then that many chunk indices | Retransmission request, only for stream packet format 2. |
| `0x0c` | 10005 | 6 bytes: item, type/action nibbles, `int32` value | Generic setting, e.g. the work mode on cameras that have one. |
| `0x0d` / `0x0e` | 10005 | | Resolution list / select, only on cameras with capability `0x40`. |
| `0x0f` | 10005 | `uint32` port | Port for file notifications (not used by otoscopes). |
| `0x10` / `0x11` | 10005 | 12 bytes, three `int32`: auto exposure, unknown, absolute exposure | Get / set exposure. The N30 Pro answers the getter with error 2 (**verified**). |
| `0x12` / `0x13` | 10005 | 40 bytes, ten `int32`: brightness, contrast, hue, saturation, sharpness, gamma, auto white balance, white balance temperature, backlight compensation, gain | Get / set image parameters. The N30 Pro answers the getter with error 2 (**verified**). |
| `0x14` | 10006 | `uint16` port, length 0 | Registers the client's announcement port. |
| `0x15` | 10006 | 1 byte | Low-power mode, only on cameras with device flag bit 2. The N30 Pro answers error 2. |
| `0x16` | 10006 | `uint16` video port, length 0 | Registers the video port and starts the stream. **Verified**, see [Starting the stream](#starting-the-stream). |
| `0x1a` | 10005 | year `uint16`, then month, day, hour, minute, second | Set the clock, only if device flag bit 4 asks for it. |
| `0x1b` | 10005 | 1 byte | Record status sync, only with device flag bit 6. |

On the N30 Pro the protocol offers no setting for JPEG quality, frame rate or
resolution.

## Device info

Type `0x01` with no payload, e.g. `ee ff ee ff 01 00 01 00 01 00 00 00`. The reply
payload is exactly 128 bytes:

| Offset | Size | Field | LMECHN N30 Pro |
|---|---|---|---|
| 0x00 | 1 | unknown | `05` |
| 0x01 | 32 | vendor tag, NUL-terminated | `YPC` |
| 0x21 | 32 | product string | `BK7231U-XRH-FBPRO` |
| 0x41 | 16 | firmware version | `HKV48B` |
| 0x51 | 32 | SSID | `LMECHN-59F77` |
| 0x71 | 6 | MAC | `82:77:8a:01:58:74` |
| 0x77 | 2 | power: battery % = `>> 9`, bit 8 = charging | 97 %, not charging |
| 0x79 | 1 | capability bits | `0x03` |
| 0x7a | 1 | work mode 1 | |
| 0x7b | 1 | work mode 2 (signed) | |
| 0x7c | 1 | flags: bit 2 low-power mode, bit 4 wants clock set, bit 6 record status sync | `0x00` |

The product string is a firmware identifier; it does not necessarily name the chip
inside.

Capability bits: `0x01` LED, `0x02` gravity sensor, `0x04` motor, `0x08` AD effect,
`0x10` work mode, `0x20` no battery, `0x40` resolution switching, `0x80` gravity in
MJPEG. The N30 Pro reports `0x03`: an LED and a gravity sensor, and a fixed 480x480
resolution.

A read-only client should send nothing but `0x01`.

## LED

Type `0x0a`, payload length 3, both ways:

| Byte | Field |
|---|---|
| 0 | LED number (low nibble), `0` get / `1` set (high nibble) |
| 1 | state: 0 off, 1 on, 2 blink, 3 breathe, 4 follows button |
| 2 | brightness 0–100 |

`01 00 00` reads LED 1, `11 01 64` switches it on at 100 %. On the N30 Pro the LED is on
at 100 % from power-up (**verified**); EarCam still sends the set command once after
connecting, for cameras that start with it off.

## Starting the stream

**Verified** on the N30 Pro:

1. Bind a UDP socket and let the OS pick the port.
2. Read the port back (`getsockname`).
3. Send type `0x16` from that socket to camera port 10006, with the port as a
   little-endian `uint16`:

   ```
   ee ff ee ff <id:2> 16 00 01 00 00 00 <port:2>
   ```

   The length field is `0` although two payload bytes follow; the camera expects this.

Frames start arriving on the registered socket immediately, from source port 10006.
EarCam sends the registration again whenever frames stop for more than 1.5 s, each
time preceded by a GetDeviceInfo as a liveness check.

## Announcements

Once a client is associated, the camera sends a 29-byte type `0x09` message to client
port 10007 every second (**verified**):

```
ee ff ee ff 00 00 09 00 00 00 11 00 02 0c 01 00 00 00 00 58 …
```

EarCam does not use them; they are the only traffic that arrives before the stream is
registered, which makes them a useful sign of life. The payload is believed to be laid
out as follows (not verified; the sample would give a battery value of 6, so that field
may be a level rather than a percentage):

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | subtype: 1 status, 2 status with buttons, 3 discovery |
| 1 | 2 | bit 0 charging, bits 1–7 battery, bit 8 sensor ok, bits 9–15 low-power-off |
| 3 | 1 | work mode |
| 4 | 1 | motor level (signed) |
| 5 | 2 | subtype 2: 4-bit rolling counters for the photo, video, zoom-out and zoom-in buttons |
| 7 | 2 | subtype 2: last two MAC bytes |
| 9 | 1 | subtype 2: frozen (low nibble), mirror (high nibble) |
| 16 | 1 | subtype 2: camera status (low nibble), picture change id (high nibble) |

## Stream chunks

JPEG frames are split across UDP datagrams, each chunk a 16-byte header followed by the
payload. The N30 Pro sends one chunk per datagram with a 1280-byte payload and streams
480x480 at a steady ~17 fps, 16–25 KB per JPEG (**verified**). Several chunks may be
packed back to back in one datagram, with payloads up to 1456 bytes; EarCam parses that
case too.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | packet format: `1` (this layout) |
| 1 | 1 | chunk index (8-bit, wraps) |
| 2 | 1 | frame index (8-bit, wraps) |
| 3 | 1 | `1` on the frame's last chunk |
| 4 | 1 | total chunks in the frame (some firmware sets it only on the last chunk) |
| 5 | 1 | flags: bit 0 gravity present, bit 3 temperature/humidity at offset 6 instead |
| 6 | 4 | gravity vector, packed (see [Orientation](#orientation)) |
| 10 | 2 | last two bytes of the camera's MAC |
| 12 | 2 | frame width |
| 14 | 2 | frame height |

The N30 Pro sends format `1` and flags `0x01` on every chunk (**verified**). EarCam takes
the gravity vector only from chunks with `flags & 0x09 == 0x01`. Other packet formats
exist in the family: `6` has a 28-byte header (payload from offset `0x1c`), `5` carries
audio, and `2` supports retransmission requests (type `0x0b`). EarCam handles format `1`
only.

### Reassembly

The chunk index is a **global** 8-bit counter that keeps running across frames (a frame
of 15 chunks might be `112..126`), so a frame's first index is only known from its last
chunk: `first = last - total + 1` (mod 256). Chunks are therefore parked by index until
the last one arrives, and a frame is complete when every index in that range is
present. Counting offsets from the first chunk *seen* breaks on reordered first chunks
and on duplicates.

The frame index wraps every 256 frames (~15 s at 17 fps). A frame that lost a chunk must
be dropped within a few frames: left in place, its stale chunks get merged into the next
frame with the same index 256 frames later, which then completes one chunk early, leaves
its real last chunk behind as the next stale entry, and keeps the corruption going
indefinitely (observed as an undecodable frame every ~15 s).

### Resolution

480x480 is the native resolution of the N30 Pro, not a mode: the camera does not
advertise resolution switching (capability `0x40`).

## Orientation

Chunk offset 6 holds a little-endian `uint32` with three 10-bit fields, each a 9-bit
magnitude with the sign in bit 9:

| Bits | Axis |
|---|---|
| 0–9 | z |
| 10–19 | y |
| 20–29 | x |

Reading the same bytes as three `int16` yields garbage. The vector is gravity, about 60
units at 1 g. **Verified:** `x` runs along the probe and stays constant while it is
rolled; `y` and `z` sweep a circle. Roll is `atan2(z, y)`, zero in the rest position and
increasing when the probe is turned counter-clockwise as seen from behind. The sensor
image comes out a quarter turn off (bottom on the left), so EarCam draws it at
`-90° - roll`. The camera does not rotate the image itself.

When the probe points nearly straight up or down, gravity runs along `x` and the roll is
undefined. EarCam computes the tilt `atan(|x| / hypot(y, z))` and keeps the previous
angle beyond 85°. Below that it eases towards the new angle, but holds still while the
change stays within a deadband of at least 3° that widens as `1 rad / hypot(y, z)`, the
angle one sensor unit is worth near the vertical.
