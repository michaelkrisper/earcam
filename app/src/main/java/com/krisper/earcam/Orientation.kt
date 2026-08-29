package com.krisper.earcam

import kotlin.math.abs
import kotlin.math.atan2

/**
 * Turns the per-frame orientation readings into a screen rotation.
 *
 * The camera reports three signed 16-bit axes in every chunk header. Which pair maps to
 * the roll around the probe axis is not documented anywhere; [angleDegrees] assumes x/y
 * and is the single place to adjust once calibrated against real hardware.
 */
object Orientation {

    fun angleDegrees(x: Int, y: Int): Float {
        if (x == 0 && y == 0) return 0f
        return Math.toDegrees(atan2(y.toDouble(), x.toDouble())).toFloat()
    }

    /**
     * Low-pass filter over the shortest angular path, so the image eases into place
     * instead of jittering on sensor noise and never spins the long way round 180°.
     */
    fun smooth(current: Float, target: Float, factor: Float = 0.25f): Float {
        var delta = (target - current) % 360f
        if (delta > 180f) delta -= 360f
        if (delta < -180f) delta += 360f
        if (abs(delta) < 0.5f) return target
        return current + delta * factor
    }
}
