package com.krisper.earcam

/**
 * Reassembles JPEG frames from out-of-order UDP chunks.
 *
 * The chunk index is a global 8-bit counter that keeps running across frames, so a
 * frame's first index is only known once its final chunk arrives: that one carries
 * [Protocol.Chunk.totalChunks], and the frame spans `last - total + 1 .. last`. Until
 * then each chunk is parked in a slot keyed by its index, which makes arrival order,
 * duplicates and wrap-around irrelevant. A frame is complete when every one of its
 * chunks is present, not when enough bytes have arrived.
 *
 * The frame index is 8 bits as well and comes round again every 256 frames (~15 s), so
 * a frame that lost a chunk must not linger: anything more than [maxConcurrentFrames]
 * behind the newest frame is dropped, or its stale chunks would be mixed into the
 * frame that reuses its index.
 *
 * Frames and their buffers are recycled, so steady-state streaming allocates nothing.
 */
class FrameAssembler(private val maxConcurrentFrames: Int = 4) {

    class Frame {
        var index = 0
        /** The reassembled JPEG, valid up to [acquired] once the frame is returned. */
        var buf = ByteArray(128 * 1024)
        var acquired = 0
        var total = 0
        var width = 0
        var height = 0
        var posX = 0
        var posY = 0
        var posZ = 0

        internal val slots = ByteArray(256 * Protocol.CHUNK_PAYLOAD)
        internal val sizes = IntArray(256)
        internal var received = 0
        internal var lastChunk = -1

        internal fun reset(index: Int) {
            this.index = index
            sizes.fill(0)
            received = 0
            lastChunk = -1
            total = 0
            acquired = 0
        }
    }

    private val frames = LinkedHashMap<Int, Frame>()
    private val spare = ArrayDeque<Frame>()
    private var returned: Frame? = null
    private var newest = -1
    private var lateChunks = 0

    /**
     * Feeds one chunk; returns the frame if this chunk completed it. The returned frame
     * is recycled by the next call, so it has to be consumed before feeding more.
     */
    fun add(chunk: Protocol.Chunk, data: ByteArray): Frame? {
        returned?.let(spare::addLast)
        returned = null

        val frame = frames[chunk.frameIndex] ?: run {
            // A straggler for a frame that already completed or was given up on. Should
            // the camera restart its counter instead, follow it after a burst of these.
            if (newest >= 0 && ((newest - chunk.frameIndex) and 0xff) < 128 && ++lateChunks < 256) return null
            lateChunks = 0
            newest = chunk.frameIndex
            newFrame(chunk.frameIndex)
        }

        val idx = chunk.chunkIndex
        if (frame.sizes[idx] == 0 && chunk.payloadSize > 0) {
            System.arraycopy(data, chunk.payloadOffset, frame.slots, idx * Protocol.CHUNK_PAYLOAD, chunk.payloadSize)
            frame.sizes[idx] = chunk.payloadSize
            frame.received++
        }
        frame.width = chunk.width
        frame.height = chunk.height
        frame.posX = chunk.posX
        frame.posY = chunk.posY
        frame.posZ = chunk.posZ
        if (chunk.isLastChunk && chunk.totalChunks > 0) {
            frame.lastChunk = idx
            frame.total = chunk.totalChunks
        }

        if (frame.lastChunk < 0 || frame.received < frame.total) return null
        val first = frame.lastChunk - frame.total + 1
        var size = 0
        for (i in 0 until frame.total) {
            val s = frame.sizes[(first + i) and 0xff]
            if (s == 0) return null
            size += s
        }

        if (frame.buf.size < size) frame.buf = ByteArray(size)
        var offset = 0
        for (i in 0 until frame.total) {
            val slot = (first + i) and 0xff
            val s = frame.sizes[slot]
            System.arraycopy(frame.slots, slot * Protocol.CHUNK_PAYLOAD, frame.buf, offset, s)
            offset += s
        }
        frame.acquired = size
        frames.remove(frame.index)
        returned = frame
        return frame
    }

    private fun newFrame(index: Int): Frame {
        val it = frames.values.iterator()
        while (it.hasNext()) {
            val old = it.next()
            if (((index - old.index) and 0xff) >= maxConcurrentFrames || frames.size >= maxConcurrentFrames) {
                it.remove()
                spare.addLast(old)
            }
        }
        val frame = (spare.removeFirstOrNull() ?: Frame()).apply { reset(index) }
        frames[index] = frame
        return frame
    }
}
