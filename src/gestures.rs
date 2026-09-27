//! Touch gestures on touchscreens (Android): pinch zooms the whole picture, circle
//! included, around the fingers; one finger pans; a double tap resets.
//!
//! Pure state machine over pointer events, so it is tested without a device.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const MAX_ZOOM: f32 = 4.0;
/// The circle is this much wider than the screen; see [`crate::render`].
pub const MASK_OVERSCAN: f32 = 1.1;

const DOUBLE_TAP: Duration = Duration::from_millis(300);
/// How far a finger may move and still count as a tap, in pixels.
const TAP_SLOP: f32 = 24.0;

/// Magnification and the circle centre's offset from the screen centre, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    pub scale: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Zoom {
            scale: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

impl Zoom {
    /// Zooms to `scale` so that the point under (`fx`, `fy`) stays put.
    pub fn around(self, fx: f32, fy: f32, scale: f32, width: f32, height: f32) -> Zoom {
        let next = scale.clamp(1.0, MAX_ZOOM);
        let f = next / self.scale;
        let (cx, cy) = (fx - width / 2.0, fy - height / 2.0);
        Zoom {
            scale: next,
            pan_x: cx - (cx - self.pan_x) * f,
            pan_y: cy - (cy - self.pan_y) * f,
        }
        .clamped(width, height)
    }

    pub fn panned(self, dx: f32, dy: f32, width: f32, height: f32) -> Zoom {
        Zoom {
            pan_x: self.pan_x + dx,
            pan_y: self.pan_y + dy,
            ..self
        }
        .clamped(width, height)
    }

    /// Keeps the screen centre on the magnified circle.
    fn clamped(self, width: f32, height: f32) -> Zoom {
        let max = width.min(height) / 2.0 * MASK_OVERSCAN * (self.scale - 1.0);
        let len = self.pan_x.hypot(self.pan_y);
        if len > max && len > 0.0 {
            Zoom {
                pan_x: self.pan_x * max / len,
                pan_y: self.pan_y * max / len,
                ..self
            }
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

/// Tracks the pointers and turns their motion into [`Zoom`] changes.
#[derive(Default)]
pub struct Gestures {
    pub zoom: Zoom,
    pointers: BTreeMap<u64, (f32, f32)>,
    /// Where a potential tap started, while only one finger is down and it stays put.
    tap_start: Option<(Instant, f32, f32)>,
    last_tap: Option<(Instant, f32, f32)>,
}

impl Gestures {
    /// Feeds one pointer event; returns true if the zoom changed.
    pub fn touch(
        &mut self,
        id: u64,
        phase: Phase,
        x: f32,
        y: f32,
        now: Instant,
        (width, height): (f32, f32),
    ) -> bool {
        let before = self.zoom;
        match phase {
            Phase::Down => {
                self.pointers.insert(id, (x, y));
                self.tap_start = (self.pointers.len() == 1).then_some((now, x, y));
            }
            Phase::Move => {
                let Some(&old) = self.pointers.get(&id) else {
                    return false;
                };
                let (old_centre, old_spread) = self.centre_and_spread();
                self.pointers.insert(id, (x, y));
                let (centre, spread) = self.centre_and_spread();
                if let Some((_, tx, ty)) = self.tap_start
                    && (x - tx).hypot(y - ty) > TAP_SLOP
                {
                    self.tap_start = None;
                }
                if self.pointers.len() >= 2 && old_spread > 0.0 {
                    let scale = self.zoom.scale * spread / old_spread;
                    self.zoom = self.zoom.around(centre.0, centre.1, scale, width, height);
                    self.zoom = self.zoom.panned(
                        centre.0 - old_centre.0,
                        centre.1 - old_centre.1,
                        width,
                        height,
                    );
                } else if self.pointers.len() == 1 {
                    self.zoom = self.zoom.panned(x - old.0, y - old.1, width, height);
                }
            }
            Phase::Up => {
                self.pointers.remove(&id);
                if let Some((start, tx, ty)) = self.tap_start.take()
                    && now - start < DOUBLE_TAP
                {
                    let double = self.last_tap.is_some_and(|(t, lx, ly)| {
                        now - t < DOUBLE_TAP * 2 && (tx - lx).hypot(ty - ly) < TAP_SLOP * 2.0
                    });
                    if double {
                        self.zoom = Zoom::default();
                        self.last_tap = None;
                    } else {
                        self.last_tap = Some((now, tx, ty));
                    }
                }
            }
        }
        self.zoom != before
    }

    /// Centroid of the pointers and their mean distance from it.
    fn centre_and_spread(&self) -> ((f32, f32), f32) {
        let n = self.pointers.len().max(1) as f32;
        let (sx, sy) = self
            .pointers
            .values()
            .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x, b + y));
        let c = (sx / n, sy / n);
        let spread = self
            .pointers
            .values()
            .map(|&(x, y)| (x - c.0).hypot(y - c.1))
            .sum::<f32>()
            / n;
        (c, spread)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1000.0;
    const H: f32 = 2000.0;

    fn at(ms: u64) -> Instant {
        // A fixed base keeps the tests deterministic.
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    #[test]
    fn pinch_zooms_around_the_fingers() {
        let mut g = Gestures::default();
        g.touch(1, Phase::Down, 400.0, 1000.0, at(0), (W, H));
        g.touch(2, Phase::Down, 600.0, 1000.0, at(0), (W, H));
        assert!(g.touch(2, Phase::Move, 800.0, 1000.0, at(50), (W, H)));
        // Fingers spread 200 -> 400 px: twice the magnification.
        assert!((g.zoom.scale - 2.0).abs() < 1e-3, "{:?}", g.zoom);
    }

    #[test]
    fn zoom_is_clamped() {
        let z = Zoom::default().around(500.0, 1000.0, 10.0, W, H);
        assert_eq!(z.scale, MAX_ZOOM);
        let z = Zoom::default().around(500.0, 1000.0, 0.2, W, H);
        assert_eq!(z.scale, 1.0);
    }

    #[test]
    fn pan_is_impossible_unzoomed_and_bounded_zoomed() {
        let z = Zoom::default().panned(300.0, 0.0, W, H);
        assert_eq!(z, Zoom::default());
        let z = Zoom {
            scale: 2.0,
            ..Zoom::default()
        }
        .panned(10_000.0, 0.0, W, H);
        let max = W / 2.0 * MASK_OVERSCAN;
        assert!((z.pan_x - max).abs() < 1e-3, "{z:?}");
    }

    #[test]
    fn zooming_keeps_the_focus_point_still() {
        let focus = (700.0, 1200.0);
        let z = Zoom::default().around(focus.0, focus.1, 2.0, W, H);
        // Image point under the focus before (relative to the unzoomed centre)...
        let before = (focus.0 - W / 2.0, focus.1 - H / 2.0);
        // ...is where the focus maps to after undoing pan and zoom.
        let after = (
            (focus.0 - W / 2.0 - z.pan_x) / z.scale,
            (focus.1 - H / 2.0 - z.pan_y) / z.scale,
        );
        assert!((after.0 * z.scale + z.pan_x - before.0).abs() < 1e-3);
        assert!((after.1 * z.scale + z.pan_y - before.1).abs() < 1e-3);
    }

    #[test]
    fn double_tap_resets() {
        let mut g = Gestures {
            zoom: Zoom {
                scale: 3.0,
                pan_x: 50.0,
                pan_y: 0.0,
            },
            ..Default::default()
        };
        g.touch(1, Phase::Down, 500.0, 1000.0, at(0), (W, H));
        g.touch(1, Phase::Up, 500.0, 1000.0, at(80), (W, H));
        g.touch(1, Phase::Down, 505.0, 1002.0, at(200), (W, H));
        assert!(g.touch(1, Phase::Up, 505.0, 1002.0, at(260), (W, H)));
        assert_eq!(g.zoom, Zoom::default());
    }

    #[test]
    fn slow_taps_do_not_reset() {
        let mut g = Gestures {
            zoom: Zoom {
                scale: 3.0,
                ..Zoom::default()
            },
            ..Default::default()
        };
        g.touch(1, Phase::Down, 500.0, 1000.0, at(0), (W, H));
        g.touch(1, Phase::Up, 500.0, 1000.0, at(80), (W, H));
        g.touch(1, Phase::Down, 500.0, 1000.0, at(2000), (W, H));
        g.touch(1, Phase::Up, 500.0, 1000.0, at(2080), (W, H));
        assert_eq!(g.zoom.scale, 3.0);
    }

    #[test]
    fn one_finger_pans_when_zoomed() {
        let mut g = Gestures {
            zoom: Zoom {
                scale: 2.0,
                ..Zoom::default()
            },
            ..Default::default()
        };
        g.touch(1, Phase::Down, 500.0, 1000.0, at(0), (W, H));
        g.touch(1, Phase::Move, 540.0, 980.0, at(16), (W, H));
        assert_eq!((g.zoom.pan_x, g.zoom.pan_y), (40.0, -20.0));
    }
}
