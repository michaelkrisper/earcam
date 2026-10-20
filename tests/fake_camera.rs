//! End-to-end test of the stream against a simulated camera on localhost: port
//! registration, chunked UDP delivery, reassembly, JPEG decode and the gravity vector.
//! Covers everything between the WiFi switch and the window.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use earcam::protocol::{
    self, ChunkHeader, Gravity, TYPE_GET_DEVICE_INFO, TYPE_LED, TYPE_SET_PIC_PORT, parse_command,
};
use earcam::stream::{self, Camera, Exchange, Route, Slot};

const FRAME: &[u8] = include_bytes!("data/frame.jpg");
/// What the LMECHN firmware puts in each datagram.
const PAYLOAD: usize = 1280;

/// How the simulated camera misbehaves.
#[derive(Clone, Copy, Default)]
struct Quirks {
    /// Ignore this many registrations before streaming, as a camera still booting would.
    ignore_registrations: usize,
    /// Send each frame's chunks in reverse order.
    reverse_chunks: bool,
    /// Send every chunk twice.
    duplicate_chunks: bool,
}

struct FakeCamera {
    camera: Camera,
    registrations: Arc<AtomicUsize>,
    led_on: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl FakeCamera {
    /// Answers device queries and streams `frames` frames of [`FRAME`] with `gravity`
    /// to whichever port registers.
    fn start(gravity: Gravity, frames: usize, quirks: Quirks) -> FakeCamera {
        let command = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let stream = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        command
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let camera = Camera {
            ip: Ipv4Addr::LOCALHOST,
            command_port: command.local_addr().unwrap().port(),
            stream_port: stream.local_addr().unwrap().port(),
        };
        let registrations = Arc::new(AtomicUsize::new(0));
        let led_on = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

        let (s, led) = (stop.clone(), led_on.clone());
        thread::spawn(move || {
            let mut buf = [0u8; 256];
            while !s.load(Ordering::Relaxed) {
                if let Ok((n, from)) = command.recv_from(&mut buf) {
                    let h = parse_command(&buf[..n]).expect("command header");
                    let reply = match h.ty {
                        TYPE_GET_DEVICE_INFO => device_info(h.id),
                        TYPE_LED => {
                            if buf[12] == 0x11 {
                                led.store(true, Ordering::SeqCst);
                            }
                            protocol::command(TYPE_LED, h.id, &buf[12..15], 3)
                        }
                        // Anything else is unsupported, as the image parameters may be.
                        _ => {
                            let mut nak = ack(h.ty, h.id);
                            nak[9] = 2;
                            nak
                        }
                    };
                    command.send_to(&reply, from).unwrap();
                }
            }
        });

        let (s, count) = (stop.clone(), registrations.clone());
        thread::spawn(move || {
            let mut buf = [0u8; 256];
            while !s.load(Ordering::Relaxed) {
                let Ok((n, from)) = stream.recv_from(&mut buf) else {
                    continue;
                };
                let h = parse_command(&buf[..n]).expect("command header");
                assert_eq!(h.ty, TYPE_SET_PIC_PORT);
                // Header says length 0, yet the port follows.
                assert_eq!(n, protocol::COMMAND_HEADER_SIZE + 2);
                let port = u16::from_le_bytes([buf[12], buf[13]]);
                stream.send_to(&ack(h.ty, h.id), from).unwrap();
                if count.fetch_add(1, Ordering::SeqCst) < quirks.ignore_registrations {
                    continue;
                }
                let to = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                send_frames(&stream, to, gravity, frames, quirks);
            }
        });

        FakeCamera {
            camera,
            registrations,
            led_on,
            stop,
        }
    }
}

impl Drop for FakeCamera {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A GetDeviceInfo reply as the LMECHN unit sends it: 128-byte payload with vendor,
/// product string, firmware and SSID at fixed offsets.
fn device_info(id: u16) -> Vec<u8> {
    let mut payload = [0u8; protocol::DEVICE_INFO_SIZE];
    payload[0x01..0x04].copy_from_slice(b"YPC");
    payload[0x21..0x32].copy_from_slice(b"BK7231U-XRH-FBPRO");
    payload[0x41..0x47].copy_from_slice(b"HKV48B");
    payload[0x51..0x5a].copy_from_slice(b"SIMCAM-01");
    payload[0x77..0x79].copy_from_slice(&(80u16 << 9 | 0x100).to_le_bytes());
    payload[0x79] = protocol::CAP_LIGHT | 0x02;
    protocol::command(TYPE_GET_DEVICE_INFO, id, &payload, payload.len() as u16)
}

fn ack(ty: u16, id: u16) -> Vec<u8> {
    protocol::command(ty, id, &[], 0)
}

/// Cuts [`FRAME`] into chunks like the firmware does: a global chunk counter, and only the
/// last chunk carrying the total.
fn send_frames(socket: &UdpSocket, to: SocketAddr, gravity: Gravity, frames: usize, q: Quirks) {
    let mut chunk_index = 250u8; // wraps within the first frame
    for frame_index in 0..frames {
        let pieces: Vec<&[u8]> = FRAME.chunks(PAYLOAD).collect();
        let mut datagrams: Vec<Vec<u8>> = pieces
            .iter()
            .enumerate()
            .map(|(i, piece)| {
                let last = i == pieces.len() - 1;
                let header = ChunkHeader {
                    chunk_index,
                    frame_index: frame_index as u8,
                    is_last: last,
                    total: if last { pieces.len() as u8 } else { 0 },
                    flags: 1,
                    gravity,
                    width: 480,
                    height: 480,
                };
                chunk_index = chunk_index.wrapping_add(1);
                [&header.encode()[..], piece].concat()
            })
            .collect();
        if q.reverse_chunks {
            datagrams.reverse();
        }
        for d in &datagrams {
            socket.send_to(d, to).unwrap();
            if q.duplicate_chunks {
                socket.send_to(d, to).unwrap();
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Runs the stream against `fake` until `want` frames arrived or 10 s passed; returns
/// the last frame and how many arrived.
fn receive(fake: &FakeCamera, want: usize) -> (Slot, usize) {
    let running = Arc::new(AtomicBool::new(true));
    let exchange = Arc::new(Exchange::default());
    let (tx, rx) = mpsc::channel();
    let worker = {
        let (camera, running, exchange) = (fake.camera, running.clone(), exchange.clone());
        thread::spawn(move || {
            stream::run(&camera, &Route::default(), &running, &exchange, || {
                let _ = tx.send(());
            })
        })
    };

    let mut frames = 0;
    let mut slot = Slot::default();
    while frames < want && rx.recv_timeout(Duration::from_secs(10)).is_ok() {
        frames += 1;
        exchange.take(&mut slot);
    }
    running.store(false, Ordering::Relaxed);
    worker.join().unwrap().expect("stream failed");
    (slot, frames)
}

#[test]
fn streams_and_decodes_frames() {
    let gravity = Gravity {
        x: -3,
        y: 70,
        z: -12,
    };
    let fake = FakeCamera::start(gravity, 5, Quirks::default());
    let (slot, frames) = receive(&fake, 5);

    assert_eq!(frames, 5);
    assert_eq!((slot.width, slot.height), (480, 480));
    assert_eq!(slot.rgb.len(), 480 * 480 * 3);
    assert_eq!(slot.gravity, gravity);
    assert!(fake.led_on.load(Ordering::SeqCst), "LED switched on");
    // Top-left marker in the fixture is near-white, the ellipse in the middle dark red.
    let px = |x: usize, y: usize| &slot.rgb[(y * 480 + x) * 3..][..3];
    assert!(
        px(50, 40).iter().all(|&c| c > 200),
        "marker: {:?}",
        px(50, 40)
    );
    let centre = px(240, 240);
    assert!(centre[0] > 90 && centre[1] < 70, "centre: {centre:?}");
}

#[test]
fn reassembles_reordered_and_duplicated_chunks() {
    let quirks = Quirks {
        reverse_chunks: true,
        duplicate_chunks: true,
        ..Quirks::default()
    };
    let fake = FakeCamera::start(Gravity::default(), 3, quirks);
    let (slot, frames) = receive(&fake, 3);
    assert_eq!(frames, 3);
    assert_eq!((slot.width, slot.height), (480, 480));
}

#[test]
fn re_registers_until_the_camera_streams() {
    let quirks = Quirks {
        ignore_registrations: 1,
        ..Quirks::default()
    };
    let fake = FakeCamera::start(Gravity::default(), 1, quirks);
    let (_, frames) = receive(&fake, 1);
    assert_eq!(frames, 1);
    assert!(fake.registrations.load(Ordering::SeqCst) >= 2);
}

#[test]
fn probe_identifies_the_camera() {
    let fake = FakeCamera::start(Gravity::default(), 0, Quirks::default());
    let info = stream::probe(&fake.camera, &Route::default(), Duration::from_secs(3))
        .expect("the simulated camera answers");
    assert_eq!(info.product, "BK7231U-XRH-FBPRO");
    assert_eq!(info.ssid, "SIMCAM-01");
    assert_eq!((info.battery_percent, info.charging), (80, true));
}

#[test]
fn probe_gives_up_on_silence() {
    // Something listens, but never answers: like a home router at 192.168.1.1.
    let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = silent.local_addr().unwrap().port();
    let camera = Camera {
        ip: Ipv4Addr::LOCALHOST,
        command_port: port,
        stream_port: port,
    };
    let started = std::time::Instant::now();
    assert!(stream::probe(&camera, &Route::default(), Duration::from_millis(900)).is_none());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn stops_promptly_without_a_camera() {
    // Nothing listens on these ports: the stream keeps retrying until told to stop.
    let camera = Camera {
        ip: Ipv4Addr::LOCALHOST,
        command_port: 9,
        stream_port: 9,
    };
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let worker = thread::spawn(move || {
        stream::run(&camera, &Route::default(), &r, &Exchange::default(), || {})
    });
    thread::sleep(Duration::from_millis(300));
    running.store(false, Ordering::Relaxed);
    let started = std::time::Instant::now();
    worker
        .join()
        .unwrap()
        .expect("a silent camera is not an error");
    assert!(started.elapsed() < Duration::from_secs(3));
}
