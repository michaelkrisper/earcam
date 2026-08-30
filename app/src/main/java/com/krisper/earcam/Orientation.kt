package com.krisper.earcam

import kotlin.math.abs
import kotlin.math.atan2

/**
 * Turns the per-frame gravity vector into a screen rotation.
 *
 * Calibrated on hardware: x runs along the probe and stays put while it is rolled, y and
 * z sweep a circle of constant magnitude. Roll is therefore `atan2(z, y)`, zero with the
 * probe in its rest position, and the image is turned by its negative to cancel it. The sensor image itself comes out a quarter turn off,
 * hence [UPRIGHT_OFFSET].
 */
object Orientation {

    const val UPRIGHT_OFFSET = -90f

    fun angleDegrees(y: Int, z: Int): Float {
        if (y == 0 && z == 0) return 0f
        return Math.toDegrees(atan2(z.toDouble(), y.toDouble())).toFloat()
    }

    /**
     * Low-pass filter over the shortest angular path, so the image eases into place
     * instead of jittering on sensor noise and never spins the long way round 180°.
     *
     * Inside [deadband] the image is held completely still. Easing towards a noisy
     * target still tracks that noise, only slower, and a probe that trembles a degree
     * or two while being held is the normal case rather than the exception.
     */
    fun smooth(
        current: Float,
        target: Float,
        factor: Float = 0.12f,
        deadband: Float = 3f,
    ): Float {
        var delta = (target - current) % 360f
        if (delta > 180f) delta -= 360f
        if (delta < -180f) delta += 360f
        if (abs(delta) < deadband) return current
        return current + delta * factor
    }
}
