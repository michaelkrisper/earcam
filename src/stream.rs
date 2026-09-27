//! UDP receive, JPEG reassembly and decode on one thread, handed to the renderer through
//! a triple buffer.
//!
//! The camera streams only after the client registers a receive port (type 0x16 to port
//! 10006); it is re-sent whenever frames stall. Socket reads block anyway, a 480x480 JPEG
//! decodes in a couple of milliseconds, and the kernel buffers the next burst meanwhile.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::JpegDecoder;

use crate::assembler::Assembler;
use crate::protocol::{self, parse_chunk};

/// A decoded RGB frame and the gravity reading that came with it. Recycled, never shared.
#[derive(Default)]
pub struct Slot {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub y: i32,
    pub z: i32,
}

/// Triple buffer: each side owns one slot and swaps it against the middle one, so neither
/// waits and a frame the renderer did not get to is simply overwritten by the next.
#[derive(Default)]
pub struct Exchange {
    middle: Mutex<(Slot, bool)>,
}

impl Exchange {
    fn publish(&self, slot: &mut Slot) {
        let mut m = self.middle.lock().unwrap();
        std::mem::swap(&mut m.0, slot);
        m.1 = true;
    }

    /// Swaps `front` for the newest published slot; false if there is none.
    pub fn take(&self, front: &mut Slot) -> bool {
        let mut m = self.middle.lock().unwrap();
        if !m.1 {
            return false;
        }
        m.1 = false;
        std::mem::swap(&mut m.0, front);
        true
    }
}

/// Streams until `running` goes false, calling `on_frame` after each published frame.
pub fn run(running: &AtomicBool, exchange: &Exchange, on_frame: impl Fn()) -> std::io::Result<()> {
    let control = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    // Port 0: the camera is told afterwards which port it ended up on.
    let video = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    control.set_read_timeout(Some(Duration::from_millis(1500)))?;
    video.set_read_timeout(Some(Duration::from_millis(1000)))?;
    // Frames arrive as bursts of ~1.3 KB datagrams; the default buffer drops some, and
    // every dropped datagram costs a whole frame.
    set_receive_buffer(&video, 4 << 20);

    let device = Ipv4Addr::from(protocol::DEVICE_IP);
    let video_port = video.local_addr()?.port();
    let mut assembler = Assembler::new(4);
    let mut buf = vec![0u8; 64 * 1024];
    let mut reply = [0u8; 1024];
    let mut back = Slot::default();
    let mut msg_id = 0u16;

    let mut last_request: Option<Instant> = None;
    let mut last_frame: Option<Instant> = None;
    let mut stats_start = Instant::now();
    let mut stats_frames = 0u32;
    let mut decode_time = Duration::ZERO;

    // Sends a command and waits for the ack.
    let mut command = |req: Vec<u8>, port: u16| {
        let ty = req[6];
        if let Err(e) = control.send_to(&req, SocketAddrV4::new(device, port)) {
            eprintln!("cmd 0x{ty:02x} -> {port}: {e}");
            return;
        }
        match control.recv(&mut reply) {
            Ok(n) => eprintln!("cmd 0x{ty:02x} -> {port}: {n} bytes, err={}", reply[9]),
            Err(_) => eprintln!("cmd 0x{ty:02x} -> {port}: no reply"),
        }
    };

    while running.load(Ordering::Relaxed) {
        // (Re-)request the stream if it never started or stalled. The device query in
        // front mirrors the stock client and doubles as a liveness check.
        let stalled = last_frame.is_none_or(|t| t.elapsed() > Duration::from_millis(1500));
        if stalled && last_request.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            last_request = Some(Instant::now());
            msg_id = msg_id.wrapping_add(1);
            command(
                protocol::command(protocol::TYPE_GET_DEVICE_INFO, msg_id, &[], 0),
                protocol::COMMAND_PORT,
            );
            msg_id = msg_id.wrapping_add(1);
            command(
                protocol::set_pic_port(video_port, msg_id),
                protocol::STREAM_PORT,
            );
        }

        let n = match video.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e),
        };
        let data = &buf[..n];

        // A datagram can carry several chunks back to back.
        let mut offset = 0;
        while let Some(chunk) = parse_chunk(data, offset) {
            offset = chunk.payload_offset + chunk.payload_size;
            let Some(frame) = assembler.add(&chunk, data) else {
                continue;
            };

            let t0 = Instant::now();
            if let Err(e) = decode(&frame.jpeg, &mut back) {
                eprintln!(
                    "undecodable frame {}, {} bytes: {e}",
                    frame.index,
                    frame.jpeg.len()
                );
                continue;
            }
            decode_time += t0.elapsed();
            back.y = frame.y;
            back.z = frame.z;
            let (w, h, size, x, y, z) = (
                back.width,
                back.height,
                frame.jpeg.len(),
                frame.x,
                frame.y,
                frame.z,
            );
            exchange.publish(&mut back);
            on_frame();

            let now = Instant::now();
            last_frame = Some(now);
            stats_frames += 1;
            let elapsed = now - stats_start;
            if elapsed >= Duration::from_secs(1) {
                eprintln!(
                    "{:.1} fps, decode {:.2} ms, {w}x{h}, {size} bytes, x={x} y={y} z={z}",
                    stats_frames as f64 / elapsed.as_secs_f64(),
                    decode_time.as_secs_f64() * 1e3 / stats_frames as f64,
                );
                stats_start = now;
                stats_frames = 0;
                decode_time = Duration::ZERO;
            }
        }
    }
    Ok(())
}

fn decode(jpeg: &[u8], into: &mut Slot) -> Result<(), zune_jpeg::errors::DecodeErrors> {
    let mut decoder = JpegDecoder::new(ZCursor::new(jpeg));
    decoder.decode_headers()?;
    let info = decoder.info().unwrap();
    // Grows once, then decodes into the same buffer every frame.
    into.rgb.resize(decoder.output_buffer_size().unwrap(), 0);
    decoder.decode_into(&mut into.rgb)?;
    into.width = info.width as usize;
    into.height = info.height as usize;
    Ok(())
}

fn set_receive_buffer(socket: &UdpSocket, bytes: libc::c_int) {
    // Capped by net.core.rmem_max; best effort.
    unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&raw const bytes).cast(),
            size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}
