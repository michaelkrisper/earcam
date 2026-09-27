//! Wire format of the i4season "bkCamera" WiFi otoscope family (Suear, LMECHN, inskam, ...).
//!
//! Reverse-engineered from the stock LMECHN app and its libWifiCamera.so; the full
//! write-up is docs/protocol.md in github.com/michaelkrisper/earcam.

pub const DEVICE_IP: [u8; 4] = [192, 168, 1, 1];
pub const COMMAND_PORT: u16 = 10005;
pub const STREAM_PORT: u16 = 10006;

/// Upper bound on the payload of a single stream chunk. This firmware sends 1280 and one
/// chunk per datagram; the cap only matters if a datagram ever packs several.
pub const CHUNK_PAYLOAD: usize = 1456;
pub const CHUNK_HEADER_SIZE: usize = 16;
pub const COMMAND_HEADER_SIZE: usize = 12;

const MAGIC: u32 = 0xffee_ffee;

pub const TYPE_GET_DEVICE_INFO: u16 = 0x0001;
#[cfg(test)]
pub const TYPE_OPEN_VIDEO: u16 = 0x0004;
/// Registers the UDP port the client wants frames on; this is what actually starts the
/// stream on this firmware. Sent to [`STREAM_PORT`]. The OpenVideo (0x04) of the older
/// Suear-era clients is acked and then does nothing.
pub const TYPE_SET_PIC_PORT: u16 = 0x0016;

/// A 12-byte command header followed by `payload`.
pub fn command(ty: u16, id: u16, payload: &[u8], declared_len: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(COMMAND_HEADER_SIZE + payload.len());
    buf.extend_from_slice(&MAGIC.to_le_bytes());
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

/// One fragment of a JPEG frame: a 16-byte header followed by up to [`CHUNK_PAYLOAD`] bytes.
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub chunk_index: u8,
    pub frame_index: u8,
    pub is_last: bool,
    /// Zero until the last chunk.
    pub total: u8,
    /// Gravity vector, ~60 units at 1 g. x lies along the probe, so roll is `atan2(z, y)`.
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Unused: the JPEG carries its own size.
    #[allow(dead_code)]
    pub width: u16,
    #[allow(dead_code)]
    pub height: u16,
    pub payload_offset: usize,
    pub payload_size: usize,
}

fn sign_mag(v: u32) -> i32 {
    let mag = (v & 0x1ff) as i32;
    if v & 0x200 != 0 {
        -mag
    } else {
        mag
    }
}

/// Parses the chunk starting at `offset` in `data`, or None if less than a header remains.
pub fn parse_chunk(data: &[u8], offset: usize) -> Option<Chunk> {
    let h = data.get(offset..offset + CHUNK_HEADER_SIZE)?;
    // Three 10-bit fields (9-bit magnitude, sign in bit 9), z lowest. Reading them as
    // int16, as older write-ups do, gives garbage.
    let packed = u32::from_le_bytes([h[6], h[7], h[8], h[9]]);
    let payload_offset = offset + CHUNK_HEADER_SIZE;
    Some(Chunk {
        chunk_index: h[1],
        frame_index: h[2],
        is_last: h[3] == 1,
        total: h[4],
        z: sign_mag(packed),
        y: sign_mag(packed >> 10),
        x: sign_mag(packed >> 20),
        width: u16::from_le_bytes([h[12], h[13]]),
        height: u16::from_le_bytes([h[14], h[15]]),
        payload_offset,
        payload_size: CHUNK_PAYLOAD.min(data.len() - payload_offset),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn open_video_matches_captured_bytes() {
        let expected = [0xee, 0xff, 0xee, 0xff, 0, 0, 4, 0, 1, 0, 0, 0];
        assert_eq!(command(TYPE_OPEN_VIDEO, 0, &[], 0), expected);
    }

    #[test]
    fn set_pic_port_declares_zero_length() {
        let cmd = set_pic_port(0x1234, 2);
        assert_eq!(cmd.len(), 14);
        assert_eq!(&cmd[4..8], &[2, 0, 0x16, 0]);
        assert_eq!(&cmd[10..], &[0, 0, 0x34, 0x12]);
    }

    #[test]
    fn parse_chunk_reads_header_fields() {
        let data = chunk(7, 3, 0, 0, -5, 9, 1, CHUNK_PAYLOAD, 0);
        let c = parse_chunk(&data, 0).unwrap();
        assert_eq!((c.chunk_index, c.frame_index), (7, 3));
        assert_eq!((c.x, c.y, c.z), (-5, 9, 1));
        assert_eq!((c.width, c.height), (640, 480));
        assert_eq!(c.payload_offset, CHUNK_HEADER_SIZE);
        assert_eq!(c.payload_size, CHUNK_PAYLOAD);
    }

    #[test]
    fn parse_chunk_rejects_short_buffer() {
        assert!(parse_chunk(&[0; 8], 0).is_none());
    }

    fn enc(v: i32) -> u32 {
        if v < 0 {
            0x200 | (-v) as u32
        } else {
            v as u32
        }
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
        let mut b = vec![1, chunk_index, frame_index, last, total, 0];
        b.extend_from_slice(&(enc(x) << 20 | enc(y) << 10 | enc(z)).to_le_bytes());
        b.extend_from_slice(&0x7458u16.to_le_bytes());
        b.extend_from_slice(&640u16.to_le_bytes());
        b.extend_from_slice(&480u16.to_le_bytes());
        b.resize(CHUNK_HEADER_SIZE + payload, fill);
        b
    }
}
