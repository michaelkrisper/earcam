//! Turns the per-frame gravity vector into a screen rotation.
//!
//! Calibrated on hardware: x runs along the probe, y and z sweep a circle as it is rolled.
//! Roll is `atan2(z, y)` and the image is turned by its negative to cancel it; the sensor
//! image itself comes out a quarter turn off, hence [`UPRIGHT_OFFSET`].

use crate::protocol::Gravity;

pub const UPRIGHT_OFFSET: f32 = -90.0;

/// Beyond this tilt the probe points almost straight up or down: gravity runs along it,
/// y and z shrink to sensor noise and their angle is meaningless.
const MAX_TILT: f32 = 85.0;

/// The roll, or None if it cannot be told (see [`MAX_TILT`]); keep the previous one then.
pub fn angle_degrees(g: Gravity) -> Option<f32> {
    let r = (g.y as f32).hypot(g.z as f32);
    if r == 0.0 || (g.x.abs() as f32).atan2(r).to_degrees() > MAX_TILT {
        return None;
    }
    Some((g.z as f32).atan2(g.y as f32).to_degrees())
}

/// A step of one unit in y or z turns the roll by up to 1 rad / r, so the noise floor
/// outgrows [`DEADBAND`] as the probe tilts towards the vertical.
pub fn deadband(g: Gravity) -> f32 {
    let r = (g.y as f32).hypot(g.z as f32);
    DEADBAND.max(1f32.to_degrees() / r)
}

/// Low-pass filter over the shortest angular path, so the image eases into place instead
/// of jittering on sensor noise, and never spins the long way round. Inside `deadband`
/// the image is held completely still: a probe trembling a degree or two is the norm.
pub fn smooth(current: f32, target: f32, factor: f32, deadband: f32) -> f32 {
    let mut delta = (target - current) % 360.0;
    if delta > 180.0 {
        delta -= 360.0;
    }
    if delta < -180.0 {
        delta += 360.0;
    }
    if delta.abs() < deadband {
        return current;
    }
    current + delta * factor
}

pub const DEADBAND: f32 = 3.0;

/// The [`smooth`] factor for a step of `dt_ms`, so the easing speed does not depend on
/// how often it runs. Tuned as 0.12 per camera frame at 17 fps (~59 ms).
pub fn factor_for(dt_ms: f32) -> f32 {
    1.0 - 0.88f32.powf(dt_ms / 59.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_takes_shortest_path() {
        assert!(smooth(350.0, 10.0, 0.5, DEADBAND) > 350.0);
    }

    fn g(x: i32, y: i32, z: i32) -> Gravity {
        Gravity { x, y, z }
    }

    #[test]
    fn angle_follows_roll() {
        assert!((angle_degrees(g(0, 0, 60)).unwrap() - 90.0).abs() < 1e-3);
        assert!((angle_degrees(g(30, -60, 0)).unwrap() - 180.0).abs() < 1e-3);
    }

    #[test]
    fn angle_is_unknown_when_probe_points_up_or_down() {
        assert_eq!(angle_degrees(g(0, 0, 0)), None);
        assert_eq!(angle_degrees(g(60, 3, 3)), None);
        assert!(angle_degrees(g(60, 6, 0)).is_some());
    }

    #[test]
    fn deadband_widens_near_vertical() {
        assert_eq!(deadband(g(0, 60, 0)), DEADBAND);
        assert!(deadband(g(59, 6, 0)) > 9.0);
    }
}
