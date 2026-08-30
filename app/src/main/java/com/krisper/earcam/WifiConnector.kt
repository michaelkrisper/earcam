package com.krisper.earcam

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiNetworkSpecifier
import android.os.PatternMatcher
import android.util.Log

/**
 * Joins the camera's access point without taking the whole phone offline.
 *
 * The camera is a hard-wired AP (its firmware has no station mode), so the phone has to
 * associate with it. Requesting the network through [ConnectivityManager] instead of the
 * system WiFi settings keeps the result app-scoped: mobile data and the home WiFi stay
 * the default route for everything else, and the [Network] handle is bound per socket
 * in [CameraStream].
 */
class WifiConnector(context: Context) {

    private val cm = context.getSystemService(ConnectivityManager::class.java)
    private var callback: ConnectivityManager.NetworkCallback? = null

    /**
     * Requests the camera network. Android shows a system picker that the user has to
     * confirm, so this issues exactly one request and waits for that interaction —
     * cycling through candidate SSIDs would cancel the picker out from under the user.
     */
    fun connect(onAvailable: (Network) -> Unit, onFailed: () -> Unit) {
        disconnect()
        Log.i(TAG, "requesting network with prefix $SSID_PREFIX")

        // The camera's AP is open — verified by scan: "LMECHN-59F77 [ESS]" with no
        // RSN/WPA flags. Setting a passphrase here would exclude the only match.
        val specifier = WifiNetworkSpecifier.Builder()
            .setSsidPattern(PatternMatcher(SSID_PREFIX, PatternMatcher.PATTERN_PREFIX))
            .build()

        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .setNetworkSpecifier(specifier)
            .build()

        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                Log.i(TAG, "connected to camera network")
                onAvailable(network)
            }

            override fun onUnavailable() {
                Log.w(TAG, "camera network unavailable (declined or not found)")
                onFailed()
            }

            override fun onLost(network: Network) {
                Log.w(TAG, "camera network lost")
            }
        }
        callback = cb
        // Generous: the timeout must outlast the user reading and confirming the picker.
        cm.requestNetwork(request, cb, PICKER_TIMEOUT_MS)
    }

    fun disconnect() {
        callback?.let { runCatching { cm.unregisterNetworkCallback(it) } }
        callback = null
    }

    companion object {
        const val TAG = "EarCam"

        /** Covers "LMECHN-59F77" and any sibling unit of the same brand. */
        private const val SSID_PREFIX = "LMECHN"

        private const val PICKER_TIMEOUT_MS = 120_000
    }
}
