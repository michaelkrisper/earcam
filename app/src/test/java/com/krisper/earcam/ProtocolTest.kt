package com.krisper.earcam

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class ProtocolTest {

    @Test
    fun openVideoMatchesCapturedBytes() {
        // Captured from the stock client: magic, id=0, type=4, unk=1, err=0, len=0.
        val expected = byteArrayOf(
            0xee.toByte(), 0xff.toByte(), 0xee.toByte(), 0xff.toByte(),
            0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00,
        )
        assertTrue(expected.contentEquals(Protocol.command(Protocol.TYPE_OPEN_VIDEO)))
    }

    @Test
    fun commandIdIsLittleEndian() {
        val cmd = Protocol.command(Protocol.TYPE_GET_DEVICE_INFO, id = 0x0102)
        assertEquals(0x02.toByte(), cmd[4])
        assertEquals(0x01.toByte(), cmd[5])
        assertEquals(0x01.toByte(), cmd[6]) // type low byte
    }

    @Test
    fun parseChunkReadsHeaderFields() {
        val data = chunk(chunkIndex = 7, frameIndex = 3, last = 0, total = 0, x = -5, y = 9, z = 1)
        val c = Protocol.parseChunk(data, 0, data.size)
        assertNotNull(c)
        c!!
        assertEquals(7, c.chunkIndex)
        assertEquals(3, c.frameIndex)
        assertEquals(-5, c.posX)
        assertEquals(9, c.posY)
        assertEquals(1, c.posZ)
        assertEquals(640, c.width)
        assertEquals(480, c.height)
        assertEquals(Protocol.CHUNK_HEADER_SIZE, c.payloadOffset)
        assertEquals(Protocol.CHUNK_PAYLOAD, c.payloadSize)
    }

    @Test
    fun parseChunkRejectsShortBuffer() {
        assertNull(Protocol.parseChunk(ByteArray(8), 0, 8))
    }

    @Test
    fun assemblerCompletesFrameOnFinalChunk() {
        val assembler = FrameAssembler()
        // Two full chunks, then a final short one announcing a total of 3.
        val a = chunk(chunkIndex = 0, frameIndex = 1, last = 0, total = 0)
        assertNull(assembler.add(Protocol.parseChunk(a, 0, a.size)!!, a))
        val b = chunk(chunkIndex = 1, frameIndex = 1, last = 0, total = 0)
        assertNull(assembler.add(Protocol.parseChunk(b, 0, b.size)!!, b))

        val tail = chunk(chunkIndex = 2, frameIndex = 1, last = 1, total = 3, payload = 100)
        val frame = assembler.add(Protocol.parseChunk(tail, 0, tail.size)!!, tail)
        assertNotNull(frame)
        assertEquals(3, frame!!.total)
        assertEquals(Protocol.CHUNK_PAYLOAD * 2 + 100, frame.acquired)
    }

    @Test
    fun assemblerHandlesWrappedChunkCounter() {
        val assembler = FrameAssembler()
        // A frame that starts at index 254 and wraps past 255 to 0.
        val a = chunk(chunkIndex = 254, frameIndex = 9, last = 0, total = 0)
        assembler.add(Protocol.parseChunk(a, 0, a.size)!!, a)
        val b = chunk(chunkIndex = 255, frameIndex = 9, last = 0, total = 0)
        assembler.add(Protocol.parseChunk(b, 0, b.size)!!, b)
        val c = chunk(chunkIndex = 0, frameIndex = 9, last = 1, total = 3, payload = 10)
        val frame = assembler.add(Protocol.parseChunk(c, 0, c.size)!!, c)
        assertNotNull(frame)
        assertEquals(Protocol.CHUNK_PAYLOAD * 2 + 10, frame!!.acquired)
    }

    @Test
    fun assemblerRecyclesFrameBuffers() {
        val assembler = FrameAssembler()
        fun single(frameIndex: Int): FrameAssembler.Frame {
            val c = chunk(chunkIndex = 0, frameIndex = frameIndex, last = 1, total = 1, payload = 10)
            return assembler.add(Protocol.parseChunk(c, 0, c.size)!!, c)!!
        }
        val first = single(1).buf
        assertTrue(first === single(2).buf)
    }

    @Test
    fun assemblerOrdersChunksByIndexNotArrival() {
        val assembler = FrameAssembler()
        val b = chunk(chunkIndex = 113, frameIndex = 5, last = 0, total = 0, fill = 2)
        val c = chunk(chunkIndex = 114, frameIndex = 5, last = 1, total = 3, payload = 10, fill = 3)
        val a = chunk(chunkIndex = 112, frameIndex = 5, last = 0, total = 0, fill = 1)
        assertNull(assembler.add(Protocol.parseChunk(b, 0, b.size)!!, b))
        assertNull(assembler.add(Protocol.parseChunk(c, 0, c.size)!!, c))
        assertNull(assembler.add(Protocol.parseChunk(b, 0, b.size)!!, b)) // duplicate
        val frame = assembler.add(Protocol.parseChunk(a, 0, a.size)!!, a)!!
        assertEquals(Protocol.CHUNK_PAYLOAD * 2 + 10, frame.acquired)
        assertEquals(1, frame.buf[0].toInt())
        assertEquals(2, frame.buf[Protocol.CHUNK_PAYLOAD].toInt())
        assertEquals(3, frame.buf[Protocol.CHUNK_PAYLOAD * 2].toInt())
    }

    @Test
    fun assemblerDropsFramesThatFellBehind() {
        val assembler = FrameAssembler()
        fun feed(chunkIndex: Int, frameIndex: Int, last: Int = 0, total: Int = 0) =
            chunk(chunkIndex, frameIndex, last, total, payload = 10).let {
                assembler.add(Protocol.parseChunk(it, 0, it.size)!!, it)
            }
        // Frame 0 loses a chunk; its final chunk stays parked.
        feed(chunkIndex = 1, frameIndex = 0, last = 1, total = 2)
        // Frames 1..4 go by, so frame 0 is dropped.
        for (f in 1..4) assertNotNull(feed(chunkIndex = 10 + f, frameIndex = f, last = 1, total = 1))
        // A new frame reusing index 0 must not complete with the stale chunk.
        assertNull(feed(chunkIndex = 0, frameIndex = 0))
    }

    @Test
    fun smoothTakesShortestPath() {
        // 350 -> 10 must move forwards by 20, not backwards by 340.
        val next = Orientation.smooth(350f, 10f, factor = 0.5f)
        assertTrue("expected >350, got $next", next > 350f)
    }

    @Test
    fun angleIsZeroWhenSensorIsFlat() {
        assertEquals(0f, Orientation.angleDegrees(0, 0), 0.001f)
        assertEquals(90f, Orientation.angleDegrees(0, 100), 0.001f)
    }

    private fun chunk(
        chunkIndex: Int,
        frameIndex: Int,
        last: Int,
        total: Int,
        x: Int = 0,
        y: Int = 0,
        z: Int = 0,
        payload: Int = Protocol.CHUNK_PAYLOAD,
        fill: Int = 0,
    ): ByteArray {
        val buf = ByteBuffer.allocate(Protocol.CHUNK_HEADER_SIZE + payload)
            .order(ByteOrder.LITTLE_ENDIAN)
        buf.put(1)
        buf.put(chunkIndex.toByte())
        buf.put(frameIndex.toByte())
        buf.put(last.toByte())
        buf.put(total.toByte())
        buf.put(0)
        buf.putInt((signMag(x) shl 20) or (signMag(y) shl 10) or signMag(z))
        buf.putShort(0x7458)
        buf.putShort(640)
        buf.putShort(480)
        while (buf.hasRemaining()) buf.put(fill.toByte())
        return buf.array()
    }

    private fun signMag(v: Int): Int = if (v < 0) 0x200 or -v else v
}
