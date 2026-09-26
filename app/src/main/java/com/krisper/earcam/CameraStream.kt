package com.krisper.earcam

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Network
import android.os.Process
import android.util.Log
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.SocketTimeoutException

/**
 * Opens the video stream on the camera and decodes frames into [exchange].
 *
 * Verified against the hardware (see docs/protocol.md): the client binds a socket on an
 * OS-assigned port and registers that port with the camera, which then pushes frames to
 * it. The bare OpenVideo of the older Suear-era clients is acknowledged by this firmware
 * but never starts anything.
 *
 * Receiving and decoding share one plain thread: the socket reads block anyway, a
 * 640x480 JPEG decodes in a few milliseconds, and the kernel buffers the next frame's
 * datagrams meanwhile.
 */
class CameraStream(
    private val network: Network?,
    private val exchange: FrameExchange,
) {

    @Volatile private var running = false
    @Volatile private var sockets: List<DatagramSocket> = emptyList()

    fun start() {
        running = true
        Thread(::run, "earcam-stream").start()
    }

    /** Stops the thread; closing the sockets unblocks a pending receive immediately. */
    fun stop() {
        running = false
        sockets.forEach(DatagramSocket::close)
    }

    private fun run() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_DISPLAY)
        val control = DatagramSocket()
        // Port 0: the camera is told afterwards which port it ended up on, so there is
        // nothing to gain from a fixed one.
        val video = DatagramSocket(0)
        sockets = listOf(control, video)
        try {
            network?.bindSocket(control)
            network?.bindSocket(video)
            // Frames arrive as bursts of ~1.3 KB datagrams. The default receive buffer
            // is far too small for a whole frame's burst, and every dropped datagram
            // costs a whole frame.
            runCatching { video.receiveBufferSize = 4 * 1024 * 1024 }
            video.soTimeout = 1000
            control.soTimeout = 1500
            receive(control, video)
        } catch (e: IOException) {
            // Closed by stop(), or the camera network vanished under us (ENETUNREACH).
            if (running) Log.w(WifiConnector.TAG, "stream ended: ${e.message}")
        } finally {
            control.close()
            video.close()
        }
    }

    private fun receive(control: DatagramSocket, video: DatagramSocket) {
        val addr = InetAddress.getByName(Protocol.DEVICE_IP)
        val assembler = FrameAssembler()
        val buf = ByteArray(64 * 1024)
        val packet = DatagramPacket(buf, buf.size)
        val reply = DatagramPacket(ByteArray(1024), 1024)
        // ARGB_8888 (the default) keeps the full colour depth of the JPEG; RGB_565 would
        // band the low-contrast skin tones this camera mostly sees. Mutable, so the next
        // frame can be decoded into the same pixels.
        val options = BitmapFactory.Options().apply { inMutable = true }
        var back = FrameSlot()

        var lastRequest = 0L
        var lastFrame = 0L
        var statsStart = System.currentTimeMillis()
        var statsFrames = 0
        var decodeNs = 0L
        // One message counter shared by every command, as the reference client does.
        var msgId = 0

        /** Sends a command and waits for the acknowledgement; returns its error code. */
        fun exchange(req: ByteArray, port: Int): Int {
            control.send(DatagramPacket(req, req.size, addr, port))
            return try {
                reply.length = 1024
                control.receive(reply)
                val err = reply.data[9].toInt() and 0xff
                Log.i(WifiConnector.TAG, "cmd 0x%02x -> %d: %d bytes, err=%d".format(req[6], port, reply.length, err))
                err
            } catch (e: SocketTimeoutException) {
                Log.w(WifiConnector.TAG, "cmd 0x%02x -> %d: no reply".format(req[6], port))
                -1
            }
        }

        while (running) {
            val now = System.currentTimeMillis()
            // (Re-)request the stream if it never started or stalled. Registering the
            // receive port is what actually starts it; the device query in front of it
            // mirrors the stock client and doubles as a liveness check.
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
                val t0 = System.nanoTime()
                val bmp = decode(frame, back.bitmap, options)
                decodeNs += System.nanoTime() - t0
                if (bmp == null) {
                    Log.w(WifiConnector.TAG, "undecodable frame ${frame.index}, ${frame.acquired} bytes")
                    continue
                }
                back.bitmap = bmp
                back.posX = frame.posX
                back.posY = frame.posY
                back.posZ = frame.posZ
                back = exchange.publish(back)

                // Only a decoded frame counts as "the stream is alive".
                lastFrame = System.currentTimeMillis()
                statsFrames++
                if (lastFrame - statsStart >= 1000) {
                    Log.d(
                        WifiConnector.TAG,
                        "%.1f fps, decode %.2f ms, %dx%d, %d bytes, x=%d y=%d z=%d".format(
                            statsFrames * 1000f / (lastFrame - statsStart), decodeNs / 1e6f / statsFrames,
                            bmp.width, bmp.height, frame.acquired, frame.posX, frame.posY, frame.posZ,
                        ),
                    )
                    statsStart = lastFrame
                    statsFrames = 0
                    decodeNs = 0
                }
            }
        }
    }

    private fun decode(frame: FrameAssembler.Frame, reuse: Bitmap?, options: BitmapFactory.Options): Bitmap? {
        options.inBitmap = reuse
        return try {
            BitmapFactory.decodeByteArray(frame.buf, 0, frame.acquired, options)
        } catch (e: IllegalArgumentException) {
            // The recycled bitmap does not fit this frame (the resolution changed).
            options.inBitmap = null
            BitmapFactory.decodeByteArray(frame.buf, 0, frame.acquired, options)
        }
    }

    companion object {
        /**
         * Asks the camera for its device info over [network] (or the default route when
         * null) and reports whether it answered. Blocking. Lets the app use a camera the
         * user joined manually in system settings, instead of insisting on its own request.
         */
        fun probe(network: Network?, timeoutMs: Int = 2000): Boolean =
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
