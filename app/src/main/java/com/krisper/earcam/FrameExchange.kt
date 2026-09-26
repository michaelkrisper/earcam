package com.krisper.earcam

import android.graphics.Bitmap

/** A decoded frame and the gravity reading that came with it. Recycled, never shared. */
class FrameSlot {
    var bitmap: Bitmap? = null
    var posX = 0
    var posY = 0
    var posZ = 0
}

/**
 * Triple buffer between the decoder thread and the GL thread.
 *
 * Each side owns one slot and swaps it against the one in the middle, so the decoder can
 * decode straight into a recycled bitmap without ever touching the one being uploaded,
 * neither side waits for the other, and a frame the renderer did not get to in time is
 * simply overwritten by the next one.
 */
class FrameExchange(private val onPublish: () -> Unit) {

    private var middle = FrameSlot()
    private var fresh = false

    /** Hands over a filled [slot]; returns the slot to decode into next. */
    fun publish(slot: FrameSlot): FrameSlot {
        val back = synchronized(this) {
            middle.also {
                middle = slot
                fresh = true
            }
        }
        onPublish()
        return back
    }

    /** Swaps [front] for the newest published slot, or returns null if there is none. */
    @Synchronized
    fun take(front: FrameSlot): FrameSlot? {
        if (!fresh) return null
        fresh = false
        return middle.also { middle = front }
    }
}
