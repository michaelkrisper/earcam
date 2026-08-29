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
 * system WiFi settings keeps the result app-scoped: mobile data stays the default route
 * for everything else, and the [Network] handle is bound per socket in [CameraStream].
 */
class WifiConnector(context: Context) {

    private val cm = context.getSystemService(ConnectivityManager::class.java)
    private var callback: ConnectivityManager.NetworkCallback? = null

    /**
     * Attempts in order. The observed device advertises "LMECHN-59F77"; the remaining
     * prefixes are the other brands of this OEM family, taken from the stock app's
     * Constant.java. Each prefix is tried WPA2 first, then open, because the factory
     * passphrase is not consistent across models.
     */
    private val attempts: List<Pair<String, String?>> = buildList {
        for (prefix in listOf("LMECHN", "SUEAR", "i4season", "inskam", "Yanxuan", "SUNUO")) {
            add(prefix to CAMERA_PASSPHRASE)
            add(prefix to null)
        }
    }

    private var index = 0

    fun connect(onAvailable: (Network) -> Unit, onState: (String) -> Unit, onFailed: () -> Unit) {
        index = 0
        tryNext(onAvailable, onState, onFailed)
    }

    private fun tryNext(onAvailable: (Network) -> Unit, onState: (String) -> Unit, onFailed: () -> Unit) {
        if (index >= attempts.size) {
            Log.w(TAG, "no camera AP matched any known prefix")
            onFailed()
            return
        }
        val (prefix, passphrase) = attempts[index++]
        val label = if (passphrase != null) "$prefix (WPA2)" else "$prefix (offen)"
        Log.i(TAG, "requesting network: $label")
        onState("Suche $label …")

        val specifier = WifiNetworkSpecifier.Builder()
            .setSsidPattern(PatternMatcher(prefix, PatternMatcher.PATTERN_PREFIX))
            .apply { if (passphrase != null) setWpa2Passphrase(passphrase) }
            .build()

        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .setNetworkSpecifier(specifier)
            .build()

        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                Log.i(TAG, "connected via $label")
                onAvailable(network)
            }

            override fun onUnavailable() {
                Log.i(TAG, "no match for $label")
                unregister()
                tryNext(onAvailable, onState, onFailed)
            }
        }
        callback = cb
        // Without an explicit timeout onUnavailable() is never delivered, so a
        // non-matching attempt would hang forever instead of falling through.
        cm.requestNetwork(request, cb, ATTEMPT_TIMEOUT_MS)
    }

    private fun unregister() {
        callback?.let { runCatching { cm.unregisterNetworkCallback(it) } }
        callback = null
    }

    fun disconnect() = unregister()

    companion object {
        const val TAG = "EarCam"

        /** Factory passphrase shipped by this OEM family. */
        const val CAMERA_PASSPHRASE = "12345678"

        private const val ATTEMPT_TIMEOUT_MS = 12_000
    }
}
