package com.krisper.earcam

import android.app.Activity
import android.net.Network
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import kotlin.concurrent.thread

/**
 * The whole app: one [CameraView], no layout. Progress goes to logcat only; the one
 * thing the user has to act on, the system network picker, brings its own UI.
 */
class MainActivity : Activity() {

    private lateinit var view: CameraView
    private var wifi: WifiConnector? = null
    private var stream: CameraStream? = null

    /** Bumped on every start and stop, so late callbacks from a previous run are ignored. */
    private var session = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        view = CameraView(this)
        setContentView(view)
    }

    override fun onResume() {
        super.onResume()
        view.onResume()
    }

    override fun onPause() {
        view.onPause()
        super.onPause()
    }

    override fun onStart() {
        super.onStart()
        val s = ++session
        // If the phone is already on the camera's WiFi (joined manually, or left over
        // from a previous run), use it directly rather than asking for a new network.
        thread(name = "earcam-probe") {
            val reachable = CameraStream.probe(null)
            runOnUiThread {
                if (s != session) return@runOnUiThread
                if (reachable) startStream(null) else requestCameraNetwork()
            }
        }
    }

    override fun onStop() {
        super.onStop()
        session++
        stream?.stop()
        stream = null
        wifi?.disconnect()
        wifi = null
    }

    private fun requestCameraNetwork() {
        val s = session
        wifi = WifiConnector(this).apply {
            connect(
                onAvailable = { network -> runOnUiThread { if (s == session) startStream(network) } },
                onFailed = {
                    runOnUiThread {
                        if (s == session) Toast.makeText(this@MainActivity, R.string.no_camera, Toast.LENGTH_LONG).show()
                    }
                },
            )
        }
    }

    private fun startStream(network: Network?) {
        stream?.stop()
        Log.i(WifiConnector.TAG, "starting stream on ${network ?: "default network"}")
        stream = CameraStream(network, view.exchange).apply { start() }
    }
}
