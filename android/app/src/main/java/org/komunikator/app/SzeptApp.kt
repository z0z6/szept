package org.komunikator.app

import android.app.Application
import android.net.ConnectivityManager
import android.net.Network

/**
 * The process. Holds the one [Messenger], so it lives as long as the process
 * does, not as long as a screen.
 */
class SzeptApp : Application() {
    val messenger: Messenger by lazy { Messenger(this) }

    override fun onCreate() {
        super.onCreate()
        // Switching between Wi-Fi and mobile data changes the phone's
        // addresses. Telling the core at once saves waiting for its next retry.
        val connectivity = getSystemService(ConnectivityManager::class.java)
        connectivity.registerDefaultNetworkCallback(
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) = messenger.networkChanged()

                override fun onLost(network: Network) = messenger.networkChanged()
            },
        )
    }
}
