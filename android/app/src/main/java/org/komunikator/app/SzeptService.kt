package org.komunikator.app

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat

/**
 * Keeps Szept running while no screen is open, so messages and files keep
 * arriving over Wi-Fi or mobile data until the person switches it off from
 * the notification or force-stops the app.
 *
 * Android allows this only with a visible notification. The service itself
 * does nothing else: the [Messenger] in the same process does the work.
 */
class SzeptService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val messenger = (application as SzeptApp).messenger
        if (intent?.action == ACTION_STOP) {
            messenger.shutdown()
            ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            goForeground()
        } catch (e: Exception) {
            // Android 12+ may refuse when the system restarts the service in
            // the background. The app then works while it is open.
            stopSelf()
            return START_NOT_STICKY
        }
        messenger.start()
        // If Android kills the process for memory, bring it back later.
        return START_STICKY
    }

    private fun goForeground() {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL,
                "Działanie w tle",
                NotificationManager.IMPORTANCE_MIN,
            ).apply {
                description = "Stałe powiadomienie, dzięki któremu Szept odbiera wiadomości w tle"
                setShowBadge(false)
            }
            manager.createNotificationChannel(channel)
        }
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this,
            1,
            Intent(this, SzeptService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("Szept działa w tle")
            .setContentText("Odbiera wiadomości i pliki, dopóki jest połączenie z internetem")
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setContentIntent(openApp)
            .addAction(0, "Wyłącz", stop)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceCompat.startForeground(
                this,
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_REMOTE_MESSAGING,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    companion object {
        private const val CHANNEL = "background"
        private const val NOTIFICATION_ID = 1
        private const val ACTION_STOP = "org.komunikator.app.STOP"

        /** Starts Szept in the background, or keeps it going. */
        fun start(context: Context) {
            val intent = Intent(context, SzeptService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }
    }
}
