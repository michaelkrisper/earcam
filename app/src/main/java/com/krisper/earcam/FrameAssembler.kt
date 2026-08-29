package com.krisper.earcam

/**
 * Reassembles JPEG frames from out-of-order UDP chunks.
 *
 * Chunk indices are only 8 bits and wrap around, so offsets are computed relative to
 * the first chunk seen for a frame. [Protocol.Chunk.totalChunks] stays zero until the
 * final chunk arrives, which is what tells us how long the frame actually is.
 */
class FrameAssembler(private val maxConcurrentFrames: Int = 4) {

    class Frame(val index: Int) {
        var buf = ByteArray(128 * 1024)
        var firstChunk = -1
        var chunkSize = -1
        var total = 0
        var acquired = 0
        var width = 0
        var height = 0
        var posX = 0
        var posY = 0
        var posZ = 0
        var complete = false
    }

    private val frames = LinkedHashMap<Int, Frame>()

    /** Feeds one chunk; returns the frame if this chunk completed it. */
    fun add(chunk: Protocol.Chunk, data: ByteArray): Frame? {
        val frame = frames.getOrPut(chunk.frameIndex) {
            while (frames.size >= maxConcurrentFrames) {
                val oldest = frames.keys.first()
                frames.remove(oldest)
            }
            Frame(chunk.frameIndex).also { it.firstChunk = chunk.chunkIndex }
        }
        if (frame.complete) return null

        // All chunks but the last carry a full payload, so the first non-final chunk
        // establishes the stride used to place every other chunk.
        if (!chunk.isLastChunk || frame.chunkSize < 0) {
            frame.chunkSize = chunk.payloadSize
        }

        var idx = chunk.chunkIndex
        if (idx < frame.firstChunk) idx += 256          // 8-bit counter wrapped
        val start = frame.chunkSize * (idx - frame.firstChunk)
        if (start < 0 || start + chunk.payloadSize > frame.buf.size) {
            frames.remove(chunk.frameIndex)
            return null
        }

        System.arraycopy(data, chunk.payloadOffset, frame.buf, start, chunk.payloadSize)
        frame.acquired += chunk.payloadSize
        frame.width = chunk.width
        frame.height = chunk.height
        frame.posX = chunk.posX
        frame.posY = chunk.posY
        frame.posZ = chunk.posZ
        if (chunk.totalChunks > 0) frame.total = chunk.totalChunks

        if (frame.total > 0 && frame.acquired > frame.chunkSize * (frame.total - 1)) {
            frame.complete = true
            frames.remove(chunk.frameIndex)
            return frame
        }
        return null
    }
}
