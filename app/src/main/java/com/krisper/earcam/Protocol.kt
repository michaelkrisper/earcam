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

    /** Payload bytes carried by a single stream chunk (all but the last one are full). */
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
     * Builds a 12-byte command header. Sending [TYPE_OPEN_VIDEO] to [STREAM_PORT]
     * makes the camera start pushing frames back to the sending socket.
     */
    fun command(type: Short, id: Int = 0, payload: ByteArray = ByteArray(0)): ByteArray {
        val buf = ByteBuffer.allocate(COMMAND_HEADER_SIZE + payload.size)
            .order(ByteOrder.LITTLE_ENDIAN)
        buf.putInt(MAGIC)
        buf.putShort((id and 0xffff).toShort())
        buf.putShort(type)
        buf.put(1)                      // always 1 in requests
        buf.put(0)                      // error code, 0 in requests
        buf.putShort(payload.size.toShort())
        buf.put(payload)
        return buf.array()
    }

    fun openVideo(id: Int = 0): ByteArray = command(TYPE_OPEN_VIDEO, id)

    /**
     * One fragment of a JPEG frame. A single UDP datagram may carry several of these
     * back to back, each being a 16-byte header followed by up to [CHUNK_PAYLOAD] bytes.
     */
    data class Chunk(
        val chunkIndex: Int,
        val frameIndex: Int,
        val isLastChunk: Boolean,
        val totalChunks: Int,
        /** Raw orientation sensor readings; drives image rotation. Signed 16-bit. */
        val posX: Int,
        val posY: Int,
        val posZ: Int,
        val width: Int,
        val height: Int,
        val payloadOffset: Int,
        val payloadSize: Int,
    )

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
        val posX = buf.short.toInt()
        val posY = buf.short.toInt()
        val posZ = buf.short.toInt()
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
