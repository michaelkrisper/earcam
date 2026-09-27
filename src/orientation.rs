//! Turns the per-frame gravity vector into a screen rotation.
//!
//! Calibrated on hardware: x runs along the probe, y and z sweep a circle as it is rolled.
//! Roll is `atan2(z, y)` and the image is turned by its negative to cancel it; the sensor
//! image itself comes out a quarter turn off, hence [`UPRIGHT_OFFSET`].

pub const UPRIGHT_OFFSET: f32 = -90.0;

pub fn angle_degrees(y: i32, z: i32) -> f32 {
    if y == 0 && z == 0 {
        return 0.0;
    }
    (z as f32).atan2(y as f32).to_degrees()
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

    #[test]
    fn angle_is_zero_when_sensor_is_flat() {
        assert_eq!(angle_degrees(0, 0), 0.0);
        assert!((angle_degrees(0, 100) - 90.0).abs() < 1e-3);
    }
}
