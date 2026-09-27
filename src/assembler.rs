//! Reassembles JPEG frames from out-of-order UDP chunks.
//!
//! The chunk index is a global 8-bit counter that keeps running across frames, so a
//! frame's first index is only known once its final chunk arrives: that one carries the
//! total, and the frame spans `last - total + 1 ..= last`. Until then each chunk is parked
//! in a slot keyed by its index, which makes arrival order, duplicates and wrap-around
//! irrelevant.
//!
//! The frame index is 8 bits as well and comes round every 256 frames (~15 s), so a frame
//! that lost a chunk must not linger: anything more than `max_concurrent` behind the newest
//! frame is dropped, or its stale chunks would be mixed into the frame reusing its index.
//!
//! Frames and their buffers are recycled; steady-state streaming allocates nothing.

use crate::protocol::{Chunk, CHUNK_PAYLOAD};

pub struct Frame {
    pub index: u8,
    /// The reassembled JPEG, valid once the frame has been returned.
    pub jpeg: Vec<u8>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    slots: Box<[u8]>,
    sizes: [u16; 256],
    received: usize,
    last_chunk: Option<u8>,
    total: usize,
}

impl Frame {
    fn new() -> Frame {
        Frame {
            index: 0,
            jpeg: Vec::with_capacity(128 * 1024),
            x: 0,
            y: 0,
            z: 0,
            slots: vec![0; 256 * CHUNK_PAYLOAD].into_boxed_slice(),
            sizes: [0; 256],
            received: 0,
            last_chunk: None,
            total: 0,
        }
    }

    fn reset(&mut self, index: u8) {
        self.index = index;
        self.sizes = [0; 256];
        self.received = 0;
        self.last_chunk = None;
        self.total = 0;
        self.jpeg.clear();
    }
}

pub struct Assembler {
    max_concurrent: usize,
    /// In order of creation, oldest first.
    frames: Vec<Frame>,
    spare: Vec<Frame>,
    returned: Option<Frame>,
    newest: Option<u8>,
    late_chunks: u32,
}

impl Assembler {
    pub fn new(max_concurrent: usize) -> Self {
        Assembler {
            max_concurrent,
            frames: Vec::with_capacity(max_concurrent),
            spare: Vec::new(),
            returned: None,
            newest: None,
            late_chunks: 0,
        }
    }

    /// Feeds one chunk read from `data`; returns the frame if this chunk completed it.
    pub fn add(&mut self, chunk: &Chunk, data: &[u8]) -> Option<&Frame> {
        if let Some(f) = self.returned.take() {
            self.spare.push(f);
        }

        let pos = match self
            .frames
            .iter()
            .position(|f| f.index == chunk.frame_index)
        {
            Some(pos) => pos,
            None => {
                // A straggler for a frame that already completed or was given up on. Should
                // the camera restart its counter instead, follow it after a burst of these.
                if let Some(newest) = self.newest
                    && newest.wrapping_sub(chunk.frame_index) < 128
                {
                    self.late_chunks += 1;
                    if self.late_chunks < 256 {
                        return None;
                    }
                }
                self.late_chunks = 0;
                self.newest = Some(chunk.frame_index);
                self.new_frame(chunk.frame_index)
            }
        };

        let frame = &mut self.frames[pos];
        let idx = chunk.chunk_index as usize;
        if frame.sizes[idx] == 0 && chunk.payload_size > 0 {
            let src = &data[chunk.payload_offset..chunk.payload_offset + chunk.payload_size];
            frame.slots[idx * CHUNK_PAYLOAD..][..src.len()].copy_from_slice(src);
            frame.sizes[idx] = src.len() as u16;
            frame.received += 1;
        }
        frame.x = chunk.x;
        frame.y = chunk.y;
        frame.z = chunk.z;
        if chunk.is_last && chunk.total > 0 {
            frame.last_chunk = Some(chunk.chunk_index);
            frame.total = chunk.total as usize;
        }

        let last = frame.last_chunk?;
        if frame.received < frame.total {
            return None;
        }
        let first = last.wrapping_sub(frame.total as u8 - 1);
        let complete =
            (0..frame.total).all(|i| frame.sizes[first.wrapping_add(i as u8) as usize] != 0);
        if !complete {
            return None;
        }

        frame.jpeg.clear();
        for i in 0..frame.total {
            let slot = first.wrapping_add(i as u8) as usize;
            let size = frame.sizes[slot] as usize;
            frame
                .jpeg
                .extend_from_slice(&frame.slots[slot * CHUNK_PAYLOAD..][..size]);
        }
        self.returned = Some(self.frames.remove(pos));
        self.returned.as_ref()
    }

    fn new_frame(&mut self, index: u8) -> usize {
        let mut i = 0;
        while i < self.frames.len() {
            if index.wrapping_sub(self.frames[i].index) as usize >= self.max_concurrent
                || self.frames.len() >= self.max_concurrent
            {
                let old = self.frames.remove(i);
                self.spare.push(old);
            } else {
                i += 1;
            }
        }
        let mut frame = self.spare.pop().unwrap_or_else(Frame::new);
        frame.reset(index);
        self.frames.push(frame);
        self.frames.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{parse_chunk, tests::chunk};

    fn feed(a: &mut Assembler, data: &[u8]) -> Option<usize> {
        let c = parse_chunk(data, 0).unwrap();
        a.add(&c, data).map(|f| f.jpeg.len())
    }

    #[test]
    fn completes_frame_on_final_chunk() {
        let mut a = Assembler::new(4);
        assert!(feed(&mut a, &chunk(0, 1, 0, 0, 0, 0, 0, CHUNK_PAYLOAD, 0)).is_none());
        assert!(feed(&mut a, &chunk(1, 1, 0, 0, 0, 0, 0, CHUNK_PAYLOAD, 0)).is_none());
        assert_eq!(
            feed(&mut a, &chunk(2, 1, 1, 3, 0, 0, 0, 100, 0)),
            Some(CHUNK_PAYLOAD * 2 + 100)
        );
    }

    #[test]
    fn handles_wrapped_chunk_counter() {
        let mut a = Assembler::new(4);
        feed(&mut a, &chunk(254, 9, 0, 0, 0, 0, 0, CHUNK_PAYLOAD, 0));
        feed(&mut a, &chunk(255, 9, 0, 0, 0, 0, 0, CHUNK_PAYLOAD, 0));
        assert_eq!(
            feed(&mut a, &chunk(0, 9, 1, 3, 0, 0, 0, 10, 0)),
            Some(CHUNK_PAYLOAD * 2 + 10)
        );
    }

    #[test]
    fn orders_chunks_by_index_not_arrival() {
        let mut a = Assembler::new(4);
        let b = chunk(113, 5, 0, 0, 0, 0, 0, CHUNK_PAYLOAD, 2);
        let c = chunk(114, 5, 1, 3, 0, 0, 0, 10, 3);
        let first = chunk(112, 5, 0, 0, 0, 0, 0, CHUNK_PAYLOAD, 1);
        assert!(feed(&mut a, &b).is_none());
        assert!(feed(&mut a, &c).is_none());
        assert!(feed(&mut a, &b).is_none()); // duplicate
        let f = a.add(&parse_chunk(&first, 0).unwrap(), &first).unwrap();
        assert_eq!(f.jpeg.len(), CHUNK_PAYLOAD * 2 + 10);
        assert_eq!(f.jpeg[0], 1);
        assert_eq!(f.jpeg[CHUNK_PAYLOAD], 2);
        assert_eq!(f.jpeg[CHUNK_PAYLOAD * 2], 3);
    }

    #[test]
    fn drops_frames_that_fell_behind() {
        let mut a = Assembler::new(4);
        // Frame 0 loses a chunk; its final chunk stays parked.
        feed(&mut a, &chunk(1, 0, 1, 2, 0, 0, 0, 10, 0));
        for f in 1..=4u8 {
            assert!(feed(&mut a, &chunk(10 + f, f, 1, 1, 0, 0, 0, 10, 0)).is_some());
        }
        // A new frame reusing index 0 must not complete with the stale chunk.
        assert!(feed(&mut a, &chunk(0, 0, 0, 0, 0, 0, 0, 10, 0)).is_none());
    }
}
