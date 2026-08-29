package com.krisper.earcam

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Network
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.isActive
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.SocketTimeoutException
import kotlin.coroutines.coroutineContext

/** A decoded frame together with the orientation reported alongside it. */
data class CameraFrame(
    val bitmap: Bitmap,
    val posX: Int,
    val posY: Int,
    val posZ: Int,
    val width: Int,
    val height: Int,
)

/**
 * Opens the video stream on the camera and emits decoded frames.
 *
 * The camera pushes frames back to whichever socket sent it [Protocol.TYPE_OPEN_VIDEO],
 * so a single unconnected [DatagramSocket] both requests and receives the stream.
 */
class CameraStream(
    private val network: Network?,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
) {

    /**
     * Emits frames until cancelled. [conflate] keeps the renderer on the newest frame:
     * if decoding or drawing falls behind, stale frames are dropped rather than queued,
     * which keeps latency flat instead of letting it grow.
     */
    fun frames(onState: (String) -> Unit): Flow<CameraFrame> = callbackFlow {
        val addr = InetAddress.getByName(Protocol.DEVICE_IP)
        val socket = DatagramSocket()
        // Frames arrive as bursts of ~1.5 KB datagrams. The default receive buffer is far
        // too small for a 1080p burst, and every dropped datagram costs a whole frame.
        runCatching { socket.receiveBufferSize = 4 * 1024 * 1024 }
        network?.bindSocket(socket)
        socket.soTimeout = 1000

        val assembler = FrameAssembler()
        val buf = ByteArray(64 * 1024)
        val packet = DatagramPacket(buf, buf.size)
        // ARGB_8888 keeps the full colour depth of the JPEG; RGB_565 would band the
        // low-contrast skin tones this camera mostly sees.
        val options = BitmapFactory.Options().apply {
            inPreferredConfig = Bitmap.Config.ARGB_8888
        }

        var lastRequest = 0L
        var lastFrame = 0L
        var msgId = 0
        onState(STATE_CONNECTING)

        try {
            while (coroutineContext.isActive) {
                val now = System.currentTimeMillis()
                // (Re-)request the stream if it never started or stalled.
                if (now - lastFrame > 1500 && now - lastRequest > 1000) {
                    val req = Protocol.openVideo(msgId++)
                    socket.send(DatagramPacket(req, req.size, addr, Protocol.STREAM_PORT))
                    lastRequest = now
                }

                try {
                    packet.setData(buf, 0, buf.size)
                    socket.receive(packet)
                } catch (e: SocketTimeoutException) {
                    continue
                }

                lastFrame = System.currentTimeMillis()
                var offset = 0
                val end = packet.length
                // A datagram can carry several chunks back to back.
                while (offset < end) {
                    val chunk = Protocol.parseChunk(buf, offset, end) ?: break
                    offset = chunk.payloadOffset + chunk.payloadSize
                    val frame = assembler.add(chunk, buf) ?: continue
                    val bmp = BitmapFactory.decodeByteArray(frame.buf, 0, frame.acquired, options)
                        ?: continue
                    trySend(
                        CameraFrame(bmp, frame.posX, frame.posY, frame.posZ, frame.width, frame.height)
                    )
                }
            }
        } finally {
            socket.close()
        }

        awaitClose { socket.close() }
    }.flowOn(dispatcher).conflate()

    companion object {
        const val STATE_CONNECTING = "connecting"
    }
}
