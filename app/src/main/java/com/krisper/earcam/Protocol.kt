package com.krisper.earcam

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Wire format of the i4season "bkCamera" WiFi otoscope family (Suear, LMECHN, inskam, ...).
 *
 * Reconstructed from the stock LMECHN app (com.i4season.bkCamera_lmechn) and
 * libWifiCamera.so. See docs/protocol.md.
 */
object Protocol {
    const val DEVICE_IP = "192.168.1.1"
    const val COMMAND_PORT = 10005
    const val STREAM_PORT = 10006
    /** The camera announces itself here once a second, with a type 0x09 message. */
    const val NOTIFY_PORT = 10007

    /**
     * Upper bound on the payload of a single stream chunk. This firmware sends 1280 and
     * one chunk per datagram; the cap only matters if a datagram ever packs several.
     */
    const val CHUNK_PAYLOAD = 1456
    const val CHUNK_HEADER_SIZE = 16
    const val COMMAND_HEADER_SIZE = 12

    private const val MAGIC = 0xffeeffee.toInt()

    // Message types, from the MESSAGE_TYPE table in libWifiCamera.so.
    const val TYPE_GET_DEVICE_INFO: Short = 0x0001
    const val TYPE_GET_LICENSE: Short = 0x0002
    const val TYPE_OPEN_VIDEO: Short = 0x0004
    const val TYPE_CAMERA_COMMAND: Short = 0x000c

    /**
     * Registers the UDP port the client wants frames on; this is what actually starts
     * the stream on this firmware. libWifiCamera.so sends it to [STREAM_PORT], next to
     * 0x14 (same shape, audio port) and 0x15 (stream on/off, which this unit rejects
     * with error 2). None of them appear in the older Suear-era documentation.
     */
    const val TYPE_SET_PIC_PORT: Short = 0x0016

    /** Builds a 12-byte command header followed by [payload]. */
    fun command(
        type: Short,
        id: Int = 0,
        payload: ByteArray = ByteArray(0),
        declaredLength: Int = payload.size,
    ): ByteArray {
        val buf = ByteBuffer.allocate(COMMAND_HEADER_SIZE + payload.size)
            .order(ByteOrder.LITTLE_ENDIAN)
        buf.putInt(MAGIC)
        buf.putShort((id and 0xffff).toShort())
        buf.putShort(type)
        buf.put(1)                      // always 1 in requests
        buf.put(0)                      // error code, 0 in requests
        buf.putShort(declaredLength.toShort())
        buf.put(payload)
        return buf.array()
    }

    /**
     * Tells the camera which UDP port on the client should receive video. The stock
     * client declares a payload length of 0 while still appending the two port bytes,
     * so this reproduces that quirk byte for byte.
     */
    fun setPicPort(port: Int, id: Int = 0): ByteArray = command(
        TYPE_SET_PIC_PORT, id,
        byteArrayOf((port and 0xff).toByte(), ((port shr 8) and 0xff).toByte()),
        declaredLength = 0,
    )

    /**
     * One fragment of a JPEG frame. A single UDP datagram may carry several of these
     * back to back, each being a 16-byte header followed by up to [CHUNK_PAYLOAD] bytes.
     */
    data class Chunk(
        val chunkIndex: Int,
        val frameIndex: Int,
        val isLastChunk: Boolean,
        val totalChunks: Int,
        /**
         * Gravity vector from the probe's accelerometer, ~60 units at 1 g. [posX] lies
         * along the probe, so the roll around it is `atan2(posZ, posY)`.
         */
        val posX: Int,
        val posY: Int,
        val posZ: Int,
        val width: Int,
        val height: Int,
        val payloadOffset: Int,
        val payloadSize: Int,
    )

    private fun signMag(v: Int): Int {
        val mag = v and 0x1ff
        return if (v and 0x200 != 0) -mag else mag
    }

    /**
     * Parses the chunk starting at [offset] in [data], or null if fewer than
     * [CHUNK_HEADER_SIZE] bytes remain.
     */
    fun parseChunk(data: ByteArray, offset: Int, length: Int): Chunk? {
        if (length - offset < CHUNK_HEADER_SIZE) return null
        val buf = ByteBuffer.wrap(data, offset, length - offset).order(ByteOrder.LITTLE_ENDIAN)
        buf.get()                                   // unk, always 1
        val chunkIndex = buf.get().toInt() and 0xff
        val frameIndex = buf.get().toInt() and 0xff
        val lastChunk = (buf.get().toInt() and 0xff) == 1
        val totalChunks = buf.get().toInt() and 0xff  // zero until the last chunk
        buf.get()                                   // unk
        // Three 10-bit fields (9-bit magnitude, sign in bit 9) packed little-endian, the
        // layout sensor_get_xyz in libWifiCamera.so produces. Reading them as int16 gives
        // garbage, which is what the earlier documentation did.
        val packed = buf.int
        val posZ = signMag(packed)
        val posY = signMag(packed shr 10)
        val posX = signMag(packed shr 20)
        buf.short                                   // unk, 0x7458 on this unit
        val width = buf.short.toInt() and 0xffff
        val height = buf.short.toInt() and 0xffff

        val payloadOffset = offset + CHUNK_HEADER_SIZE
        val payloadSize = minOf(CHUNK_PAYLOAD, length - payloadOffset)
        return Chunk(
            chunkIndex, frameIndex, lastChunk, totalChunks,
            posX, posY, posZ, width, height,
            payloadOffset, payloadSize,
        )
    }
}
