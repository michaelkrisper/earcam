package com.krisper.earcam

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Network
import android.util.Log
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
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
 * Verified against the hardware (see docs/protocol.md): the client binds a socket on an
 * OS-assigned port and registers that port with the camera, which then pushes frames to
 * it. The bare OpenVideo of the older Suear-era clients is acknowledged by this firmware
 * but never starts anything.
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

        val control = DatagramSocket().also { network?.bindSocket(it) }

        // Port 0: the camera is told afterwards which port it ended up on, so there is
        // nothing to gain from a fixed one.
        val video = DatagramSocket(0).apply {
            network?.bindSocket(this)
            // Frames arrive as bursts of ~1.3 KB datagrams. The default receive buffer
            // is far too small for a whole frame's burst, and every dropped datagram
            // costs a whole frame.
            runCatching { receiveBufferSize = 4 * 1024 * 1024 }
            soTimeout = 1000
        }

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
        var frameCount = 0
        // One message counter shared by every command, as the reference client does.
        var msgId = 0
        onState(STATE_CONNECTING)

        /** Sends a command and waits for the acknowledgement; returns its error code. */
        fun exchange(req: ByteArray, port: Int): Int {
            control.soTimeout = 1500
            control.send(DatagramPacket(req, req.size, addr, port))
            return try {
                val reply = ByteArray(1024)
                val p = DatagramPacket(reply, reply.size)
                control.receive(p)
                val err = reply[9].toInt() and 0xff
                Log.i(WifiConnector.TAG, "cmd 0x%02x -> %d: %d bytes, err=%d".format(req[6], port, p.length, err))
                err
            } catch (e: SocketTimeoutException) {
                Log.w(WifiConnector.TAG, "cmd 0x%02x -> %d: no reply".format(req[6], port))
                -1
            }
        }

        try {
            while (coroutineContext.isActive) {
                val now = System.currentTimeMillis()
                // (Re-)request the stream if it never started or stalled. Registering the
                // receive port is what actually starts it; the device query in front of
                // it mirrors the stock client and doubles as a liveness check.
                if (now - lastFrame > 1500 && now - lastRequest > 2000) {
                    lastRequest = now
                    exchange(Protocol.command(Protocol.TYPE_GET_DEVICE_INFO, ++msgId), Protocol.COMMAND_PORT)
                    exchange(Protocol.setPicPort(video.localPort, ++msgId), Protocol.STREAM_PORT)
                }

                try {
                    packet.setData(buf, 0, buf.size)
                    video.receive(packet)
                } catch (e: SocketTimeoutException) {
                    continue
                }

                var offset = 0
                val end = packet.length
                // A datagram can carry several chunks back to back.
                while (offset < end) {
                    val chunk = Protocol.parseChunk(buf, offset, end) ?: break
                    offset = chunk.payloadOffset + chunk.payloadSize
                    val frame = assembler.add(chunk, buf) ?: continue
                    val bmp = BitmapFactory.decodeByteArray(frame.buf, 0, frame.acquired, options)
                    if (bmp == null) {
                        Log.w(WifiConnector.TAG, "undecodable frame ${frame.index}, ${frame.acquired} bytes")
                        continue
                    }
                    // Only a decoded frame counts as "the stream is alive".
                    lastFrame = System.currentTimeMillis()
                    if (frameCount++ == 0) {
                        Log.i(WifiConnector.TAG, "first frame: ${bmp.width}x${bmp.height}")
                    }
                    trySend(
                        CameraFrame(bmp, frame.posX, frame.posY, frame.posZ, frame.width, frame.height)
                    )
                }
            }
        } catch (e: java.io.IOException) {
            // The camera network vanished under us (ENETUNREACH); end the flow cleanly
            // and let the activity decide whether to reconnect.
            Log.w(WifiConnector.TAG, "stream ended: ${e.message}")
        } finally {
            control.close()
            video.close()
        }

        awaitClose {
            control.close()
            video.close()
        }
    }.flowOn(dispatcher).conflate()

    companion object {
        const val STATE_CONNECTING = "connecting"

        /**
         * Asks the camera for its device info over [network] (or the default route when
         * null) and reports whether it answered. Lets the app use a camera the user
         * joined manually in system settings, instead of insisting on its own request.
         */
        suspend fun probe(network: Network?, timeoutMs: Int = 2000): Boolean =
            withContext(Dispatchers.IO) {
                runCatching {
                    DatagramSocket().use { socket ->
                        network?.bindSocket(socket)
                        socket.soTimeout = timeoutMs
                        val req = Protocol.command(Protocol.TYPE_GET_DEVICE_INFO)
                        val addr = InetAddress.getByName(Protocol.DEVICE_IP)
                        socket.send(DatagramPacket(req, req.size, addr, Protocol.COMMAND_PORT))

                        val reply = ByteArray(512)
                        socket.receive(DatagramPacket(reply, reply.size))
                        Log.i(WifiConnector.TAG, "camera answered probe on ${network ?: "default"}")
                        true
                    }
                }.getOrElse {
                    Log.i(WifiConnector.TAG, "probe failed: ${it.javaClass.simpleName}")
                    false
                }
            }
    }
}
