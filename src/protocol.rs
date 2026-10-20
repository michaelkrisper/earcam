//! Wire format of the i4season "bkCamera" WiFi otoscope family (Suear, LMECHN, inskam, ...).
//!
//! Reverse-engineered from the stock LMECHN app and its libWifiCamera.so; the full
//! write-up is docs/protocol.md in github.com/michaelkrisper/earcam.
//!
//! Commands and their replies share a 12-byte little-endian header starting with the magic
//! `0xffeeffee`. Video arrives as JPEG frames cut into chunks, each with a 16-byte header
//! that also carries the probe's gravity vector.

use std::net::Ipv4Addr;

pub const DEVICE_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
pub const COMMAND_PORT: u16 = 10005;
pub const STREAM_PORT: u16 = 10006;

/// Upper bound on the payload of a single stream chunk. This firmware sends 1280 and one
/// chunk per datagram; the cap only matters if a datagram ever packs several.
pub const CHUNK_PAYLOAD: usize = 1456;
pub const CHUNK_HEADER_SIZE: usize = 16;
pub const COMMAND_HEADER_SIZE: usize = 12;

const MAGIC: [u8; 4] = 0xffee_ffee_u32.to_le_bytes();

pub const TYPE_GET_DEVICE_INFO: u16 = 0x0001;
/// What the older Suear-era clients send to start the stream. This firmware acks it with
/// no error and then sends nothing; kept for reference and tests.
pub const TYPE_OPEN_VIDEO: u16 = 0x0004;
/// Registers the UDP port the client wants frames on; this is what actually starts the
/// stream on this firmware. Sent to [`STREAM_PORT`].
pub const TYPE_SET_PIC_PORT: u16 = 0x0016;
/// LED state, 3-byte payload: LED number (low nibble) and 0 = get / 1 = set (high nibble),
/// state (0 off, 1 on), brightness 0-100. The reply has the same layout.
pub const TYPE_LED: u16 = 0x000a;
/// Read-only queries the stock app never makes. EarCam logs the answers once per session
/// to find out whether this firmware has them (docs/protocol.md, "Command messages").
pub const TYPE_GET_EXPOSURE: u16 = 0x0010;
pub const TYPE_GET_PARAMETERS: u16 = 0x0012;
/// [`DeviceInfo::capabilities`] bit for a controllable LED.
pub const CAP_LIGHT: u8 = 0x01;

/// A 12-byte command header followed by `payload`.
pub fn command(ty: u16, id: u16, payload: &[u8], declared_len: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(COMMAND_HEADER_SIZE + payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&id.to_le_bytes());
    buf.extend_from_slice(&ty.to_le_bytes());
    buf.push(1); // always 1 in requests
    buf.push(0); // error code, 0 in requests
    buf.extend_from_slice(&declared_len.to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Tells the camera which UDP port should receive video. The stock client declares a
/// payload length of 0 while still appending the two port bytes; reproduced byte for byte.
pub fn set_pic_port(port: u16, id: u16) -> Vec<u8> {
    command(TYPE_SET_PIC_PORT, id, &port.to_le_bytes(), 0)
}

/// Turns LED 1 on at full brightness, as the stock app does right after connecting.
pub fn led_on(id: u16) -> Vec<u8> {
    command(TYPE_LED, id, &[0x11, 1, 100], 3)
}

/// Asks for LED 1's state and brightness.
pub fn led_query(id: u16) -> Vec<u8> {
    command(TYPE_LED, id, &[0x01, 0, 0], 3)
}

/// The header of a command or of the camera's reply to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandHeader {
    pub id: u16,
    pub ty: u16,
    /// 0 on success.
    pub error: u8,
}

/// Parses a command or reply header, or None if `data` is not one (e.g. a video chunk).
pub fn parse_command(data: &[u8]) -> Option<CommandHeader> {
    let h = data.get(..COMMAND_HEADER_SIZE)?;
    if h[..4] != MAGIC {
        return None;
    }
    Some(CommandHeader {
        id: u16::from_le_bytes([h[4], h[5]]),
        ty: u16::from_le_bytes([h[6], h[7]]),
        error: h[9],
    })
}

/// Size of the GetDeviceInfo reply payload; the stock client rejects anything else.
pub const DEVICE_INFO_SIZE: usize = 128;

/// The camera's answer to [`TYPE_GET_DEVICE_INFO`]. Getting one at all is what identifies
/// a network as a camera, whatever the brand on the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Firmware vendor tag, e.g. `YPC`.
    pub vendor: String,
    /// The product string, e.g. `BK7231U-XRH-FBPRO`. It need not name the real chip (see docs/protocol.md).
    pub product: String,
    pub firmware: String,
    pub ssid: String,
    pub mac: [u8; 6],
    pub battery_percent: u8,
    pub charging: bool,
    /// `CAMCAP_*` bits, e.g. [`CAP_LIGHT`]; `0x03` (light, G-sensor) on the LMECHN unit.
    pub capabilities: u8,
}

impl DeviceInfo {
    /// Parses a complete reply (header and 128-byte payload).
    pub fn parse(reply: &[u8]) -> Option<DeviceInfo> {
        let h = parse_command(reply)?;
        if h.ty != TYPE_GET_DEVICE_INFO || h.error != 0 {
            return None;
        }
        let p = reply.get(COMMAND_HEADER_SIZE..COMMAND_HEADER_SIZE + DEVICE_INFO_SIZE)?;
        // Fixed-size, NUL-terminated fields; the vendor field has leftover bytes after it.
        let text = |range: std::ops::Range<usize>| {
            let field = &p[range];
            let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
            String::from_utf8_lossy(&field[..end]).trim().to_owned()
        };
        let power = u16::from_le_bytes([p[0x77], p[0x78]]);
        Some(DeviceInfo {
            vendor: text(0x01..0x21),
            product: text(0x21..0x41),
            firmware: text(0x41..0x51),
            ssid: text(0x51..0x71),
            mac: p[0x71..0x77].try_into().unwrap(),
            battery_percent: (power >> 9) as u8,
            charging: power & 0x100 != 0,
            capabilities: p[0x79],
        })
    }
}

impl std::fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}), firmware {}, battery {}%{}",
            self.product,
            self.vendor,
            self.firmware,
            self.battery_percent,
            if self.charging { ", charging" } else { "" },
        )
    }
}

/// The probe's gravity vector, ~60 units at 1 g. x lies along the probe, so the roll
/// around it is `atan2(z, y)`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Gravity {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Gravity {
    /// Three 10-bit fields (9-bit magnitude, sign in bit 9), z lowest. Reading them as
    /// int16, as older write-ups do, gives garbage.
    fn unpack(packed: u32) -> Gravity {
        fn field(v: u32) -> i32 {
            let mag = (v & 0x1ff) as i32;
            if v & 0x200 != 0 { -mag } else { mag }
        }
        Gravity {
            x: field(packed >> 20),
            y: field(packed >> 10),
            z: field(packed),
        }
    }

    fn pack(self) -> u32 {
        fn field(v: i32) -> u32 {
            let mag = v.unsigned_abs().min(0x1ff);
            if v < 0 { 0x200 | mag } else { mag }
        }
        field(self.x) << 20 | field(self.y) << 10 | field(self.z)
    }
}

/// The 16-byte header of one fragment of a JPEG frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkHeader {
    /// A global 8-bit counter that keeps running across frames.
    pub chunk_index: u8,
    pub frame_index: u8,
    pub is_last: bool,
    /// Number of chunks in the frame; only set on the last one.
    pub total: u8,
    /// Bit 0: gravity present; bit 3: offset 6 holds temperature/humidity instead. Read
    /// from libWifiCamera.so, not yet observed, so EarCam only logs it.
    pub flags: u8,
    pub gravity: Gravity,
    /// Informational: the JPEG carries its own size.
    pub width: u16,
    pub height: u16,
}

impl ChunkHeader {
    /// The camera's side of [`parse_chunk`], for tests and simulators.
    pub fn encode(&self) -> [u8; CHUNK_HEADER_SIZE] {
        let mut h = [0u8; CHUNK_HEADER_SIZE];
        h[0] = 1;
        h[1] = self.chunk_index;
        h[2] = self.frame_index;
        h[3] = self.is_last as u8;
        h[4] = self.total;
        h[5] = self.flags;
        h[6..10].copy_from_slice(&self.gravity.pack().to_le_bytes());
        h[10..12].copy_from_slice(&0x7458u16.to_le_bytes()); // unknown, constant on this unit
        h[12..14].copy_from_slice(&self.width.to_le_bytes());
        h[14..16].copy_from_slice(&self.height.to_le_bytes());
        h
    }
}

/// A chunk header and where its payload sits in the datagram.
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub header: ChunkHeader,
    pub payload_offset: usize,
    pub payload_size: usize,
}

/// Parses the chunk starting at `offset` in `data`, or None if less than a header remains.
/// A datagram may carry several chunks back to back.
pub fn parse_chunk(data: &[u8], offset: usize) -> Option<Chunk> {
    let h = data.get(offset..offset.checked_add(CHUNK_HEADER_SIZE)?)?;
    let payload_offset = offset + CHUNK_HEADER_SIZE;
    Some(Chunk {
        header: ChunkHeader {
            chunk_index: h[1],
            frame_index: h[2],
            is_last: h[3] == 1,
            total: h[4],
            flags: h[5],
            gravity: Gravity::unpack(u32::from_le_bytes([h[6], h[7], h[8], h[9]])),
            width: u16::from_le_bytes([h[12], h[13]]),
            height: u16::from_le_bytes([h[14], h[15]]),
        },
        payload_offset,
        payload_size: CHUNK_PAYLOAD.min(data.len() - payload_offset),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn open_video_matches_captured_bytes() {
        // Captured from the stock client: magic, id=0, type=4, unk=1, err=0, len=0.
        let expected = [0xee, 0xff, 0xee, 0xff, 0, 0, 4, 0, 1, 0, 0, 0];
        assert_eq!(command(TYPE_OPEN_VIDEO, 0, &[], 0), expected);
    }

    #[test]
    fn command_id_is_little_endian() {
        let cmd = command(TYPE_GET_DEVICE_INFO, 0x0102, &[], 0);
        assert_eq!(&cmd[4..8], &[0x02, 0x01, 0x01, 0x00]);
    }

    #[test]
    fn set_pic_port_declares_zero_length() {
        let cmd = set_pic_port(0x1234, 2);
        assert_eq!(cmd.len(), 14);
        assert_eq!(&cmd[4..8], &[2, 0, 0x16, 0]);
        assert_eq!(&cmd[10..], &[0, 0, 0x34, 0x12]);
    }

    #[test]
    fn parse_command_reads_reply() {
        let mut reply = command(TYPE_SET_PIC_PORT, 7, &[], 0);
        reply[9] = 2;
        let h = parse_command(&reply).unwrap();
        assert_eq!(
            h,
            CommandHeader {
                id: 7,
                ty: TYPE_SET_PIC_PORT,
                error: 2
            }
        );
    }

    #[test]
    fn parse_command_rejects_video_chunks() {
        assert!(parse_command(&chunk(0, 0, 0, 0, 0, 0, 0, 100, 0)).is_none());
        assert!(parse_command(&MAGIC).is_none());
    }

    #[test]
    fn parse_chunk_reads_header_fields() {
        let data = chunk(7, 3, 0, 0, -5, 9, 1, CHUNK_PAYLOAD, 0);
        let c = parse_chunk(&data, 0).unwrap();
        assert_eq!((c.header.chunk_index, c.header.frame_index), (7, 3));
        assert_eq!(c.header.gravity, Gravity { x: -5, y: 9, z: 1 });
        assert_eq!((c.header.width, c.header.height), (640, 480));
        assert_eq!(c.payload_offset, CHUNK_HEADER_SIZE);
        assert_eq!(c.payload_size, CHUNK_PAYLOAD);
    }

    #[test]
    fn parse_chunk_rejects_short_buffer() {
        assert!(parse_chunk(&[0; 8], 0).is_none());
        assert!(parse_chunk(&[0; 20], 10).is_none());
    }

    #[test]
    fn parse_chunk_caps_payload() {
        let data = chunk(0, 0, 0, 0, 0, 0, 0, CHUNK_PAYLOAD + 40, 0);
        assert_eq!(parse_chunk(&data, 0).unwrap().payload_size, CHUNK_PAYLOAD);
    }

    #[test]
    fn gravity_unpacks_sign_magnitude_fields() {
        // x = 2, y = 70, z = -12, a reading the LMECHN unit logged: sign in bit 9 of each
        // 10-bit field, not two's complement.
        let packed = (2u32 << 20) | (70 << 10) | (0x200 | 12);
        assert_eq!(
            Gravity::unpack(packed),
            Gravity {
                x: 2,
                y: 70,
                z: -12
            }
        );
    }

    #[test]
    fn gravity_round_trips_full_range() {
        for v in [-511, -60, -1, 0, 1, 60, 511] {
            let g = Gravity {
                x: v,
                y: -v,
                z: v / 2,
            };
            assert_eq!(Gravity::unpack(g.pack()), g);
        }
    }

    /// The LMECHN-59F77's reply, rebuilt from the fields logged in docs/protocol.md.
    pub(crate) fn lmechn_device_info() -> Vec<u8> {
        let mut reply = command(TYPE_GET_DEVICE_INFO, 1, &[0; DEVICE_INFO_SIZE], 128);
        let p = &mut reply[COMMAND_HEADER_SIZE..];
        p[0] = 5;
        p[0x01..0x0f].copy_from_slice(b"YPC\0ota ok\0sys");
        p[0x21..0x32].copy_from_slice(b"BK7231U-XRH-FBPRO");
        p[0x41..0x47].copy_from_slice(b"HKV48B");
        p[0x51..0x5d].copy_from_slice(b"LMECHN-59F77");
        p[0x71..0x77].copy_from_slice(&[0x82, 0x77, 0x8a, 0x01, 0x58, 0x74]);
        p[0x77..0x79].copy_from_slice(&(97u16 << 9).to_le_bytes());
        reply
    }

    #[test]
    fn parses_device_info() {
        let info = DeviceInfo::parse(&lmechn_device_info()).unwrap();
        assert_eq!(info.vendor, "YPC");
        assert_eq!(info.product, "BK7231U-XRH-FBPRO");
        assert_eq!(info.firmware, "HKV48B");
        assert_eq!(info.ssid, "LMECHN-59F77");
        assert_eq!(info.mac, [0x82, 0x77, 0x8a, 0x01, 0x58, 0x74]);
        assert_eq!((info.battery_percent, info.charging), (97, false));
        assert_eq!(
            info.to_string(),
            "BK7231U-XRH-FBPRO (YPC), firmware HKV48B, battery 97%"
        );
    }

    #[test]
    fn device_info_rejects_short_or_failed_replies() {
        let full = lmechn_device_info();
        assert!(DeviceInfo::parse(&full[..100]).is_none());
        let mut failed = full.clone();
        failed[9] = 1;
        assert!(DeviceInfo::parse(&failed).is_none());
        assert!(DeviceInfo::parse(&set_pic_port(1, 1)).is_none());
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn chunk(
        chunk_index: u8,
        frame_index: u8,
        last: u8,
        total: u8,
        x: i32,
        y: i32,
        z: i32,
        payload: usize,
        fill: u8,
    ) -> Vec<u8> {
        let header = ChunkHeader {
            chunk_index,
            frame_index,
            is_last: last == 1,
            total,
            flags: 1,
            gravity: Gravity { x, y, z },
            width: 640,
            height: 480,
        };
        let mut b = header.encode().to_vec();
        b.resize(CHUNK_HEADER_SIZE + payload, fill);
        b
    }
}
