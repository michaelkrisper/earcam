package com.krisper.earcam

import android.net.Network
import android.os.Bundle
import android.util.Log
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.lifecycleScope
import com.krisper.earcam.ui.CameraScreen
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * The whole app: the live camera image rotated upright on black.
 *
 * There is exactly one screen and no navigation, so the state lives here rather than in a
 * ViewModel — the activity is configuration-locked and outlives every frame it renders.
 */
class MainActivity : ComponentActivity() {

    private var state by mutableStateOf(CameraUiState())
    private var wifi: WifiConnector? = null
    private var streamJob: Job? = null
    private var smoothed = Orientation.UPRIGHT_OFFSET

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        setContent {
            CameraScreen(
                state = state,
                onToggleDebug = { state = state.copy(debug = !state.debug) },
            )
        }
    }

    override fun onStart() {
        super.onStart()
        state = state.copy(error = false)
        lifecycleScope.launch {
            // If the phone is already on the camera's WiFi (joined manually, or left over
            // from a previous run), use it directly rather than asking for a new network.
            setStatus(getString(R.string.searching_current))
            if (CameraStream.probe(null)) {
                startStream(null)
                return@launch
            }
            requestCameraNetwork()
        }
    }

    override fun onStop() {
        super.onStop()
        streamJob?.cancel()
        streamJob = null
        wifi?.disconnect()
        wifi = null
    }

    private fun requestCameraNetwork() {
        val connector = WifiConnector(this)
        wifi = connector
        setStatus(getString(R.string.pick_camera))
        connector.connect(
            onAvailable = { network -> lifecycleScope.launch { startStream(network) } },
            onFailed = {
                lifecycleScope.launch { state = state.copy(error = true, status = "") }
            },
        )
    }

    private fun setStatus(text: String) {
        state = state.copy(status = text)
    }

    private fun startStream(network: Network?) {
        streamJob?.cancel()
        Log.i(WifiConnector.TAG, "starting stream on ${network ?: "default network"}")
        setStatus(getString(R.string.starting_stream))
        streamJob = lifecycleScope.launch {
            CameraStream(network).frames(onState = { }).collect { frame ->
                val roll = Orientation.angleDegrees(frame.posY, frame.posZ)
                smoothed = Orientation.smooth(smoothed, Orientation.UPRIGHT_OFFSET - roll)
                state = state.copy(
                    image = frame.bitmap.asImageBitmap(),
                    angle = smoothed,
                    error = false,
                    status = "",
                    debugText = if (state.debug) {
                        "x=${frame.posX} y=${frame.posY} z=${frame.posZ} " +
                            "${frame.width}x${frame.height} ${smoothed.toInt()}°"
                    } else {
                        ""
                    },
                )
            }
        }
    }
}
