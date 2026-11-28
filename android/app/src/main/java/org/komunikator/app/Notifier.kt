package org.komunikator.app

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapShader
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Shader
import android.media.AudioAttributes
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.Person
import androidx.core.content.ContextCompat
import androidx.core.graphics.drawable.IconCompat

/** One line of a conversation as a notification shows it. */
data class NotifiedLine(
    /** Who wrote it: their identifier, name and picture. */
    val authorId: String,
    val author: String,
    val picture: Bitmap?,
    /** A short preview of the message. */
    val text: String,
    val atMs: Long,
)

/**
 * Tells the person about messages they have not seen yet: who wrote, with
 * their picture, and, unless previews are off, the start of each message,
 * as a conversation the way Android shows chats.
 *
 * On the lock screen only "new message" shows: the content appears after
 * unlocking. The sound is the person's choice, through a channel of its own
 * (Android fixes a channel's sound once it exists).
 */
class Notifier(private val context: Context) {
    private val manager = NotificationManagerCompat.from(context)

    /** The last lines of each conversation still notified, by its key. */
    private val history = mutableMapOf<String, MutableList<NotifiedLine>>()

    fun show(
        key: String,
        title: String,
        group: Boolean,
        line: NotifiedLine,
        count: Int,
        preview: Boolean,
        sound: String,
    ) {
        if (!allowed()) return
        val lines = synchronized(history) {
            val list = history.getOrPut(key) { mutableListOf() }
            list += line
            while (list.size > MAX_LINES) list.removeAt(0)
            list.toList()
        }
        val open = Intent(context, MainActivity::class.java)
            .setAction(ACTION_OPEN)
            .putExtra(EXTRA_PEER, key)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        val pending = PendingIntent.getActivity(
            context,
            key.hashCode(),
            open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val channel = channelFor(sound)
        val summary = if (count > 1) t("Nowe wiadomości: {count}", "count" to count) else t("Nowa wiadomość")
        val picture = line.picture?.let(::round)

        // What the lock screen shows: nothing from the conversation.
        val public = NotificationCompat.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("Szept")
            .setContentText(summary)
            .build()

        val builder = NotificationCompat.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_notification)
            .setNumber(count)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(public)
            .setOnlyAlertOnce(false)
            .setAutoCancel(true)
            .setContentIntent(pending)
        if (picture != null) builder.setLargeIcon(picture)

        if (preview) {
            val me = Person.Builder().setName(t("Ty")).setKey("me").build()
            val style = NotificationCompat.MessagingStyle(me)
            if (group) {
                style.setConversationTitle(title)
                style.setGroupConversation(true)
            }
            for (each in lines) {
                val person = Person.Builder()
                    .setName(each.author)
                    .setKey(each.authorId)
                    .apply { each.picture?.let { setIcon(IconCompat.createWithBitmap(round(it))) } }
                    .build()
                style.addMessage(each.text, each.atMs, person)
            }
            builder.setStyle(style)
                .setContentTitle(title)
                .setContentText(if (group) "${line.author}: ${line.text}" else line.text)
        } else {
            builder.setContentTitle(title).setContentText(summary)
        }
        try {
            manager.notify(key.hashCode(), builder.build())
        } catch (e: SecurityException) {
            // The permission was taken away in the meantime.
        }
    }

    fun cancel(key: String) {
        synchronized(history) { history.remove(key) }
        manager.cancel(key.hashCode())
    }

    /**
     * The channel that plays `sound`, made the first time it is needed.
     * Channels of other sounds go, so the system settings list one.
     */
    private fun channelFor(sound: String): String {
        val id = "messages_$sound"
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return id
        val system = context.getSystemService(NotificationManager::class.java)
        if (system.getNotificationChannel(id) == null) {
            val channel = NotificationChannel(id, t("Wiadomości"), NotificationManager.IMPORTANCE_HIGH).apply {
                description = t("Nowe wiadomości od kontaktów")
                val resource = SOUND_RESOURCES[sound]
                if (resource == null) {
                    setSound(null, null)
                } else {
                    val uri = Uri.Builder()
                        .scheme(ContentResolver.SCHEME_ANDROID_RESOURCE)
                        .authority(context.packageName)
                        .appendPath(resource.toString())
                        .build()
                    val attributes = AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_NOTIFICATION_COMMUNICATION_INSTANT)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                    setSound(uri, attributes)
                }
            }
            system.createNotificationChannel(channel)
        }
        for (old in system.notificationChannels) {
            val ours = old.id == LEGACY_CHANNEL || old.id.startsWith("messages_")
            if (ours && old.id != id) system.deleteNotificationChannel(old.id)
        }
        return id
    }

    private fun allowed(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED

    /** A picture cut to a circle, as chats show people. */
    private fun round(source: Bitmap): Bitmap {
        val side = minOf(source.width, source.height)
        val output = Bitmap.createBitmap(side, side, Bitmap.Config.ARGB_8888)
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            shader = BitmapShader(source, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP)
        }
        val canvas = Canvas(output)
        canvas.translate(-(source.width - side) / 2f, -(source.height - side) / 2f)
        canvas.drawCircle(source.width / 2f, source.height / 2f, side / 2f, paint)
        return output
    }

    companion object {
        /** The channel of earlier versions, with the system's sound. */
        const val LEGACY_CHANNEL = "messages"
        const val ACTION_OPEN = "org.komunikator.app.OPEN_CHAT"
        const val EXTRA_PEER = "peer"

        /** How many lines of one conversation a notification keeps. */
        private const val MAX_LINES = 6
    }
}
