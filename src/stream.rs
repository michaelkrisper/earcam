//! UDP receive, JPEG reassembly and decode on one thread, handed to the renderer through
//! a triple buffer.
//!
//! The camera streams only after the client registers a receive port (type 0x16 to the
//! stream port), and the registration is re-sent whenever frames stall. It goes out from
//! the video socket itself, so the frames that follow travel the reverse path of a packet
//! we sent and pass stateful firewalls (Windows' in particular) as replies.
//!
//! Socket reads block anyway, a 480x480 JPEG decodes in about 3 ms, and the kernel buffers
//! the next burst meanwhile, so one thread does everything.

use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use zune_jpeg::JpegDecoder;
use zune_jpeg::errors::DecodeErrors;
use zune_jpeg::zune_core::bytestream::ZCursor;

use crate::assembler::Assembler;
use crate::protocol::{self, DeviceInfo, Gravity, parse_chunk, parse_command};

/// Where the camera listens. Always the defaults on real hardware; tests point it at a
/// simulated camera on localhost.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub ip: Ipv4Addr,
    pub command_port: u16,
    pub stream_port: u16,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            ip: protocol::DEVICE_IP,
            command_port: protocol::COMMAND_PORT,
            stream_port: protocol::STREAM_PORT,
        }
    }
}

/// Which network the camera's sockets use. The default route everywhere except on
/// Android, where the camera is a secondary network next to mobile data and each socket
/// is bound to it, so the rest of the phone stays online.
#[derive(Debug, Clone, Copy, Default)]
pub struct Route {
    /// `Network.getNetworkHandle()` of the camera network.
    #[cfg(target_os = "android")]
    pub network: Option<u64>,
}

impl Route {
    /// Makes `socket` send and receive through this route.
    pub fn bind(&self, socket: &UdpSocket) -> io::Result<()> {
        #[cfg(target_os = "android")]
        if let Some(network) = self.network {
            use std::os::fd::AsRawFd;
            if unsafe { ndk_sys::android_setsocknetwork(network, socket.as_raw_fd()) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let _ = socket;
        Ok(())
    }

    fn socket(&self) -> io::Result<UdpSocket> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        self.bind(&socket)?;
        Ok(socket)
    }
}

/// Asks whatever answers at `camera` for its device info, re-asking every 400 ms until
/// `timeout`. A reply identifies a camera regardless of brand or network name.
pub fn probe(camera: &Camera, route: &Route, timeout: Duration) -> Option<DeviceInfo> {
    let socket = route.socket().ok()?;
    socket
        .set_read_timeout(Some(Duration::from_millis(400)))
        .ok()?;
    let query = protocol::command(protocol::TYPE_GET_DEVICE_INFO, 1, &[], 0);
    let addr = SocketAddrV4::new(camera.ip, camera.command_port);
    let deadline = Instant::now() + timeout;
    let mut reply = [0u8; 512];
    while Instant::now() < deadline {
        // Fails while the interface is still coming up; just try again.
        if socket.send_to(&query, addr).is_err() {
            std::thread::sleep(Duration::from_millis(400));
            continue;
        }
        if let Ok(n) = socket.recv(&mut reply)
            && let Some(info) = DeviceInfo::parse(&reply[..n])
        {
            return Some(info);
        }
    }
    None
}

/// A decoded RGB frame and the gravity reading that came with it. Recycled, never shared.
#[derive(Default)]
pub struct Slot {
    /// Tightly packed RGB rows, top row first.
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub gravity: Gravity,
}

/// Triple buffer between the decoder and the renderer: each side owns one slot and swaps
/// it against the middle one, so neither waits, nothing is copied, and a frame the
/// renderer did not get to in time is simply overwritten by the next.
#[derive(Default)]
pub struct Exchange {
    middle: Mutex<(Slot, bool)>,
}

impl Exchange {
    /// Hands over a filled `slot` and puts the slot to decode into next in its place.
    pub fn publish(&self, slot: &mut Slot) {
        let mut m = self.middle.lock().unwrap();
        std::mem::swap(&mut m.0, slot);
        m.1 = true;
    }

    /// Swaps `front` for the newest published slot; false if nothing new was published.
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

/// How long without a decoded frame before the stream is requested again.
const STALL: Duration = Duration::from_millis(1500);
const RETRY: Duration = Duration::from_secs(2);

/// Streams from `camera` through `route` until `running` goes false, calling `on_frame` after each frame
/// published to `exchange`. Fails only if a socket does; a silent camera is retried.
pub fn run(
    camera: &Camera,
    route: &Route,
    running: &AtomicBool,
    exchange: &Exchange,
    mut on_frame: impl FnMut(),
) -> io::Result<()> {
    let control = route.socket()?;
    // Port 0: the camera is told afterwards which port it ended up on.
    let video = route.socket()?;
    control.set_read_timeout(Some(Duration::from_millis(1500)))?;
    video.set_read_timeout(Some(Duration::from_millis(500)))?;
    // Frames arrive as bursts of ~1.3 KB datagrams; the default buffer drops some, and
    // every dropped datagram costs a whole frame. Best effort: capped by the OS.
    let _ = socket2::SockRef::from(&video).set_recv_buffer_size(4 << 20);

    let command_addr = SocketAddrV4::new(camera.ip, camera.command_port);
    let stream_addr = SocketAddrV4::new(camera.ip, camera.stream_port);
    let video_port = video.local_addr()?.port();
    let mut assembler = Assembler::new(4);
    let mut buf = vec![0u8; 64 * 1024];
    let mut reply = [0u8; 1024];
    let mut back = Slot::default();
    let mut msg_id = 0u16;
    let mut next_id = || {
        msg_id = msg_id.wrapping_add(1);
        msg_id
    };

    let mut last_request: Option<Instant> = None;
    let mut last_frame: Option<Instant> = None;
    let mut stats = Stats::new();

    while running.load(Ordering::Relaxed) {
        // (Re-)request the stream if it never started or stalled. The device query in
        // front mirrors the stock client and doubles as a liveness check.
        if last_frame.is_none_or(|t| t.elapsed() > STALL)
            && last_request.is_none_or(|t| t.elapsed() > RETRY)
        {
            last_request = Some(Instant::now());
            let query = protocol::command(protocol::TYPE_GET_DEVICE_INFO, next_id(), &[], 0);
            match control
                .send_to(&query, command_addr)
                .and_then(|_| control.recv(&mut reply))
            {
                Ok(n) => log_reply(&reply[..n], camera.command_port),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    log::warn!("camera not answering")
                }
                Err(e) => log::warn!("device info: {e}"),
            }
            let register = protocol::set_pic_port(video_port, next_id());
            if let Err(e) = video.send_to(&register, stream_addr) {
                log::warn!("register stream port: {e}");
            }
        }

        let n = match video.recv(&mut buf) {
            Ok(n) => n,
            // ConnectionReset: Windows reports an ICMP port-unreachable for an earlier
            // send this way, e.g. while the camera is still booting.
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        };
        let data = &buf[..n];
        if parse_command(data).is_some() {
            log_reply(data, camera.stream_port);
            continue;
        }

        let mut offset = 0;
        while let Some(chunk) = parse_chunk(data, offset) {
            offset = chunk.payload_offset + chunk.payload_size;
            let Some(frame) = assembler.add(&chunk, data) else {
                continue;
            };

            let t0 = Instant::now();
            if let Err(e) = decode(&frame.jpeg, &mut back) {
                log::warn!(
                    "undecodable frame {}, {} bytes: {e}",
                    frame.index,
                    frame.jpeg.len()
                );
                continue;
            }
            stats.decode += t0.elapsed();
            back.gravity = frame.gravity;
            let info = (frame.jpeg.len(), back.width, back.height, back.gravity);
            exchange.publish(&mut back);
            on_frame();

            // Only a decoded frame counts as "the stream is alive".
            last_frame = Some(Instant::now());
            stats.frame(info);
        }
    }
    Ok(())
}

/// Decodes `jpeg` into `into`, reusing its buffer: steady-state streaming allocates nothing
/// for pixels.
pub fn decode(jpeg: &[u8], into: &mut Slot) -> Result<(), DecodeErrors> {
    let mut decoder = JpegDecoder::new(ZCursor::new(jpeg));
    decoder.decode_headers()?;
    let info = decoder
        .info()
        .ok_or(DecodeErrors::FormatStatic("no header"))?;
    let size = decoder
        .output_buffer_size()
        .ok_or(DecodeErrors::FormatStatic("too large"))?;
    into.rgb.resize(size, 0);
    decoder.decode_into(&mut into.rgb)?;
    into.width = info.width as usize;
    into.height = info.height as usize;
    Ok(())
}

fn log_reply(data: &[u8], port: u16) {
    if let Some(h) = parse_command(data) {
        log::info!(
            "cmd 0x{:02x} -> {port}: {} bytes, err={}",
            h.ty,
            data.len(),
            h.error
        );
    }
}

/// Once a second: fps, decode time, frame size and the raw gravity vector, which is what
/// to look at if the rotation ever looks wrong.
struct Stats {
    start: Instant,
    frames: u32,
    decode: Duration,
}

impl Stats {
    fn new() -> Self {
        Stats {
            start: Instant::now(),
            frames: 0,
            decode: Duration::ZERO,
        }
    }

    fn frame(&mut self, (bytes, w, h, g): (usize, usize, usize, Gravity)) {
        self.frames += 1;
        let elapsed = self.start.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        log::info!(
            "{:.1} fps, decode {:.2} ms, {w}x{h}, {bytes} bytes, x={} y={} z={}",
            self.frames as f64 / elapsed.as_secs_f64(),
            self.decode.as_secs_f64() * 1e3 / self.frames as f64,
            g.x,
            g.y,
            g.z,
        );
        *self = Stats::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_hands_over_newest_frame_once() {
        let exchange = Exchange::default();
        let mut front = Slot::default();
        assert!(!exchange.take(&mut front));

        let mut back = Slot {
            width: 1,
            ..Slot::default()
        };
        exchange.publish(&mut back);
        back.width = 2;
        exchange.publish(&mut back);

        assert!(exchange.take(&mut front));
        assert_eq!(
            front.width, 2,
            "an unconsumed frame is overwritten by the next"
        );
        assert!(!exchange.take(&mut front));
    }

    #[test]
    fn exchange_recycles_buffers() {
        let exchange = Exchange::default();
        let mut back = Slot {
            rgb: vec![0; 1024],
            ..Slot::default()
        };
        let ptr = back.rgb.as_ptr();
        exchange.publish(&mut back);
        let mut front = Slot::default();
        exchange.take(&mut front);
        assert_eq!(front.rgb.as_ptr(), ptr, "slots are swapped, not copied");
    }

    #[test]
    fn decode_rejects_garbage() {
        let mut slot = Slot::default();
        assert!(decode(&[0xff, 0xd8, 0x00, 0x01], &mut slot).is_err());
    }
}
