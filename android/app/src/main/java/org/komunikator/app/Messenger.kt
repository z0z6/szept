package org.komunikator.app

import android.app.Application
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import java.io.ByteArrayOutputStream
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.komunikator_ffi.App
import uniffi.komunikator_ffi.ChatEvent
import uniffi.komunikator_ffi.Contact
import uniffi.komunikator_ffi.EventListener
import uniffi.komunikator_ffi.Media
import uniffi.komunikator_ffi.fileKind
import uniffi.komunikator_ffi.maxAvatarLen
import uniffi.komunikator_ffi.maxMediaSize
import uniffi.komunikator_ffi.normalizeId

/** Where a message is on its way. */
enum class Status {
    /** Ours, waiting for the contact to be reachable. */
    QUEUED,

    /** Ours, left this device. */
    SENT,

    /** Ours, confirmed by the contact's device. */
    DELIVERED,

    /** Theirs. */
    RECEIVED,
}

/** Where a file stands. */
enum class Transfer {
    /** Ours: the contact fetches it from this phone. */
    SHARED,

    /** Theirs, not fetched yet. */
    OFFERED,
    DOWNLOADING,
    SAVED,
    FAILED,
}

data class FileItem(
    val name: String,
    val size: Long,
    val hash: String,
    /** "program", "archive", "image", "audio", "video", "document", "other". */
    val kind: String,
    val transfer: Transfer,
    val done: Long = 0,
    /** Where the file is on this phone: ours from the start, theirs once saved. */
    val path: String? = null,
    val reason: String? = null,
    /** "voice" or "gif" for media the core fetches on its own, else null. */
    val media: String? = null,
    /** Length of a voice message. */
    val durationMs: Long = 0,
)

/** One line of a conversation: text, or a file when [file] is set. */
data class Message(
    val id: String,
    val mine: Boolean,
    val body: String,
    val atMs: Long,
    val status: Status,
    val file: FileItem? = null,
)

data class UiState(
    val ready: Boolean = false,
    val myId: String = "",
    val contacts: List<Contact> = emptyList(),
    /** Identifiers of the contacts that are connected right now. */
    val online: Set<String> = emptySet(),
    /** Conversations, keyed by the contact's identifier. */
    val messages: Map<String, List<Message>> = emptyMap(),
    /** Number of unseen messages, keyed by the contact's identifier. */
    val unread: Map<String, Int> = emptyMap(),
    /** The contact whose conversation is on screen, if any. */
    val openId: String? = null,
    val error: String? = null,
    /** The person switched Szept off from its notification. */
    val stopped: Boolean = false,
    /** Contacts' profile pictures, keyed by identifier. */
    val avatars: Map<String, ImageBitmap> = emptyMap(),
    /** Our own profile picture. */
    val myAvatar: ImageBitmap? = null,
)

/**
 * Owns the Rust core and turns its events into screen state.
 *
 * There is one per process, held by [SzeptApp], so it outlives the screens:
 * while [SzeptService] keeps the process running, messages and files keep
 * arriving with no screen open. Conversations live in memory only: they are
 * gone when the process ends. Persistent history arrives with the encrypted
 * database.
 */
class Messenger(private val application: Application) : EventListener {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()

    private val notifier = Notifier(application)

    /** Orders "sent" and "delivered" after the message they are about. */
    private val sendLock = Any()

    @Volatile
    private var core: App? = null

    @Volatile
    private var foreground = false

    /** Copies of files we offer. Offers end with the process, so do these. */
    private val outgoingDir = File(application.filesDir, "outgoing")

    /** Received files. They stay until the person deletes them. */
    val incomingDir = File(application.filesDir, "incoming")

    /** Guards starting and stopping the core. */
    private val lifecycle = Any()

    /** Goes online, unless already there. Safe to call any number of times. */
    fun start() {
        scope.launch {
            synchronized(lifecycle) {
                if (core != null) return@synchronized
                outgoingDir.deleteRecursively()
                try {
                    val app = App(application.filesDir.absolutePath, false)
                    app.start(this@Messenger)
                    core = app
                    val contacts = app.contacts()
                    val avatars = contacts.mapNotNull { contact ->
                        app.avatar(contact.id)?.let(::decodePicture)?.let { contact.id to it }
                    }.toMap()
                    _state.update {
                        it.copy(
                            ready = true,
                            stopped = false,
                            myId = app.myId(),
                            contacts = contacts,
                            avatars = avatars,
                            myAvatar = app.myAvatar()?.let(::decodePicture),
                        )
                    }
                } catch (e: Exception) {
                    fail(e)
                }
            }
        }
    }

    /**
     * Goes offline completely, as asked from the notification. Offered files
     * stop being available; conversations stay on screen until the process ends.
     */
    fun shutdown() {
        scope.launch {
            synchronized(lifecycle) {
                val app = core ?: return@synchronized
                core = null
                _state.update { it.copy(ready = false, stopped = true, online = emptySet()) }
                app.stop()
                app.destroy()
            }
        }
    }

    /** The phone switched networks: reconnect right away. */
    fun networkChanged() {
        core?.networkChanged()
    }

    /** Called by the Rust core on one of its own threads. */
    override fun onEvent(event: ChatEvent) {
        when (event) {
            is ChatEvent.Connected -> _state.update { it.copy(online = it.online + event.peer) }
            is ChatEvent.Disconnected -> _state.update { it.copy(online = it.online - event.peer) }
            is ChatEvent.Message -> receive(
                event.peer,
                Message(event.id, false, event.body, event.sentAtMs.toLong(), Status.RECEIVED),
            )
            is ChatEvent.FileOffered -> {
                val (media, duration) = describe(event.media)
                receive(
                    event.peer,
                    Message(
                        event.id,
                        false,
                        "",
                        event.sentAtMs.toLong(),
                        Status.RECEIVED,
                        FileItem(
                            name = event.name,
                            size = event.size.toLong(),
                            hash = event.hash,
                            kind = fileKind(event.name),
                            // Media are fetched by the core right away.
                            transfer = if (media != null) Transfer.DOWNLOADING else Transfer.OFFERED,
                            media = media,
                            durationMs = duration,
                        ),
                    ),
                )
            }
            is ChatEvent.AvatarChanged -> {
                val picture = core?.avatar(event.peer)?.let(::decodePicture)
                _state.update {
                    it.copy(
                        avatars = if (picture != null) {
                            it.avatars + (event.peer to picture)
                        } else {
                            it.avatars - event.peer
                        },
                    )
                }
            }
            is ChatEvent.FileProgress -> updateFile(event.peer, event.id) {
                it.copy(transfer = Transfer.DOWNLOADING, done = event.done.toLong())
            }
            is ChatEvent.FileSaved -> updateFile(event.peer, event.id) {
                it.copy(transfer = Transfer.SAVED, done = it.size, path = event.path, reason = null)
            }
            is ChatEvent.FileFailed -> updateFile(event.peer, event.id) {
                it.copy(transfer = Transfer.FAILED, reason = event.reason)
            }
            is ChatEvent.Sent -> synchronized(sendLock) { setStatus(event.peer, event.id, Status.SENT) }
            is ChatEvent.Delivered -> synchronized(sendLock) {
                setStatus(event.peer, event.id, Status.DELIVERED)
            }
        }
    }

    /** The activity reports whether it is visible. */
    fun setForeground(visible: Boolean) {
        foreground = visible
        if (visible) _state.value.openId?.let(::markRead)
    }

    /** Shows the conversation with `id`, or the contact list for `null`. */
    fun open(id: String?) {
        _state.update { it.copy(openId = id) }
        if (id != null) markRead(id)
    }

    fun addContact(alias: String, id: String) = withCore { app ->
        app.addContact(alias, id)
        _state.update { it.copy(contacts = app.contacts()) }
    }

    fun removeContact(contact: Contact) = withCore { app ->
        app.removeContact(contact.alias)
        notifier.cancel(contact.id)
        _state.update {
            it.copy(
                contacts = app.contacts(),
                online = it.online - contact.id,
                messages = it.messages - contact.id,
                unread = it.unread - contact.id,
                avatars = it.avatars - contact.id,
                openId = if (it.openId == contact.id) null else it.openId,
            )
        }
    }

    fun send(peer: String, body: String) = withCore { app ->
        synchronized(sendLock) {
            val id = app.send(peer, body)
            append(peer, Message(id, true, body, System.currentTimeMillis(), Status.QUEUED))
        }
    }

    /**
     * Offers files picked by the person. Android hands out content links,
     * so each file is first copied into the app's own storage.
     */
    fun sendFiles(peer: String, uris: List<Uri>) = withCore { app ->
        val resolver = application.contentResolver
        for (uri in uris) {
            val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
                ?: uri.lastPathSegment
                ?: "plik"
            // Each copy gets its own folder, so two files of one name never clash.
            val dir = File(outgoingDir, System.nanoTime().toString()).apply { mkdirs() }
            val copy = File(dir, name.replace('/', '_'))
            val input = resolver.openInputStream(uri) ?: error("Nie można otworzyć pliku $name")
            input.use { source -> copy.outputStream().use { source.copyTo(it) } }
            val sent = app.sendFile(peer, copy.absolutePath)
            synchronized(sendLock) {
                append(
                    peer,
                    Message(
                        sent.id,
                        true,
                        "",
                        System.currentTimeMillis(),
                        Status.QUEUED,
                        FileItem(
                            name = sent.name,
                            size = sent.size.toLong(),
                            hash = sent.hash,
                            kind = fileKind(sent.name),
                            transfer = Transfer.SHARED,
                            done = sent.size.toLong(),
                            path = copy.absolutePath,
                        ),
                    ),
                )
                early.remove(sent.id)?.let { setStatus(peer, sent.id, it) }
            }
        }
    }

    /** A fresh file for a voice recording. Recordings go when the process ends, like other offers. */
    fun newRecording(): File {
        val dir = File(outgoingDir, System.nanoTime().toString()).apply { mkdirs() }
        return File(dir, "glosowa-${System.currentTimeMillis()}.m4a")
    }

    /** Sends a finished recording. */
    fun sendVoice(peer: String, file: File, durationMs: Long) = withCore { app ->
        val media = Media.Voice(durationMs.coerceIn(0, UInt.MAX_VALUE.toLong()).toUInt())
        offerMedia(app, peer, file, media)
    }

    /**
     * Copies a GIF or sticker the keyboard handed over into the app's own
     * storage. Call it at once, on the thread that received it: the
     * keyboard's permission to read it may end as soon as that call returns.
     * Returns null for something too big or unreadable.
     */
    fun stageGif(uri: Uri, mime: String?): File? = try {
        val extension = when (mime ?: application.contentResolver.getType(uri)) {
            "image/gif" -> "gif"
            "image/webp" -> "webp"
            "image/png" -> "png"
            "image/jpeg", "image/jpg" -> "jpg"
            else -> null
        }
        if (extension == null) {
            null
        } else {
            val dir = File(outgoingDir, System.nanoTime().toString()).apply { mkdirs() }
            val copy = File(dir, "gif-${System.currentTimeMillis()}.$extension")
            val limit = maxMediaSize().toLong()
            val input = application.contentResolver.openInputStream(uri)
            val copied = input?.use { source ->
                copy.outputStream().use { sink ->
                    val buffer = ByteArray(64 * 1024)
                    var total = 0L
                    while (true) {
                        val read = source.read(buffer)
                        if (read < 0) break
                        total += read
                        if (total > limit) return@use false
                        sink.write(buffer, 0, read)
                    }
                    true
                }
            } ?: false
            if (copied) copy else null.also { copy.delete() }
        }
    } catch (e: Exception) {
        null
    }

    /** Sends a GIF copied by [stageGif]. */
    fun sendGif(peer: String, file: File) = withCore { app -> offerMedia(app, peer, file, Media.Gif) }

    private fun offerMedia(app: App, peer: String, file: File, media: Media) {
        val sent = app.sendMedia(peer, file.absolutePath, media)
        val (kind, duration) = describe(media)
        synchronized(sendLock) {
            append(
                peer,
                Message(
                    sent.id,
                    true,
                    "",
                    System.currentTimeMillis(),
                    Status.QUEUED,
                    FileItem(
                        name = sent.name,
                        size = sent.size.toLong(),
                        hash = sent.hash,
                        kind = fileKind(sent.name),
                        transfer = Transfer.SHARED,
                        done = sent.size.toLong(),
                        path = file.absolutePath,
                        media = kind,
                        durationMs = duration,
                    ),
                ),
            )
            early.remove(sent.id)?.let { setStatus(peer, sent.id, it) }
        }
    }

    /**
     * Sets our profile picture from one the person picked: cut to a square
     * in the middle and scaled down until it fits in a single message.
     */
    fun setAvatar(uri: Uri) = withCore { app ->
        val picture = loadSquare(uri, 256) ?: error("Nie udało się otworzyć tego zdjęcia.")
        val limit = maxAvatarLen().toLong()
        var bytes = ByteArray(0)
        for ((size, quality) in listOf(256 to 86, 256 to 72, 192 to 70, 128 to 65)) {
            val scaled = if (size == picture.width) picture else Bitmap.createScaledBitmap(picture, size, size, true)
            bytes = ByteArrayOutputStream().also { scaled.compress(Bitmap.CompressFormat.JPEG, quality, it) }
                .toByteArray()
            if (bytes.size <= limit) break
        }
        app.setAvatar(bytes)
        _state.update { it.copy(myAvatar = decodePicture(bytes)) }
    }

    fun removeAvatar() = withCore { app ->
        app.setAvatar(ByteArray(0))
        _state.update { it.copy(myAvatar = null) }
    }

    /** Opens a picture the right way up, cut to a `side` × `side` square. */
    private fun loadSquare(uri: Uri, side: Int): Bitmap? {
        val full = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            // Follows the photo's orientation and decodes it already smaller.
            ImageDecoder.decodeBitmap(ImageDecoder.createSource(application.contentResolver, uri)) { decoder, info, _ ->
                val shorter = minOf(info.size.width, info.size.height)
                if (shorter > side * 2) {
                    val ratio = side * 2f / shorter
                    decoder.setTargetSize((info.size.width * ratio).toInt(), (info.size.height * ratio).toInt())
                }
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            }
        } else {
            val resolver = application.contentResolver
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
            var sample = 1
            while (minOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= side * 2) sample *= 2
            val options = BitmapFactory.Options().apply { inSampleSize = sample }
            resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) }
        } ?: return null
        val shorter = minOf(full.width, full.height)
        val square = Bitmap.createBitmap(full, (full.width - shorter) / 2, (full.height - shorter) / 2, shorter, shorter)
        // JPEG has no transparency: put transparent pictures on white.
        val result = Bitmap.createBitmap(side, side, Bitmap.Config.ARGB_8888)
        android.graphics.Canvas(result).apply {
            drawColor(android.graphics.Color.WHITE)
            drawBitmap(
                square,
                null,
                android.graphics.Rect(0, 0, side, side),
                android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG),
            )
        }
        return result
    }

    /** Starts or resumes fetching a file the contact offered. */
    fun download(peer: String, id: String) = withCore { app ->
        val media = _state.value.messages[peer]?.firstOrNull { it.id == id }?.file?.media
        // Voice messages and GIFs live with the core's own media.
        app.download(id, if (media != null) app.mediaDir() else incomingDir.absolutePath)
        updateFile(peer, id) { it.copy(transfer = Transfer.DOWNLOADING, reason = null) }
    }

    /** Checks scanned or pasted text. Returns the identifier, or null. */
    fun parseId(text: String): String? = try {
        normalizeId(text)
    } catch (e: Exception) {
        null
    }

    fun clearError() = _state.update { it.copy(error = null) }

    private fun decodePicture(bytes: ByteArray): ImageBitmap? =
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()

    private fun describe(media: Media?): Pair<String?, Long> = when (media) {
        is Media.Voice -> "voice" to media.durationMs.toLong()
        is Media.Gif -> "gif" to 0L
        null -> null to 0L
    }

    private fun updateFile(peer: String, id: String, change: (FileItem) -> FileItem) = _state.update { state ->
        val conversation = state.messages[peer] ?: return@update state
        val updated = conversation.map { message ->
            val file = message.file
            if (message.id == id && file != null) message.copy(file = change(file)) else message
        }
        state.copy(messages = state.messages + (peer to updated))
    }

    private fun receive(peer: String, message: Message) {
        val seen = foreground && _state.value.openId == peer
        var fresh = false
        _state.update {
            val conversation = it.messages[peer].orEmpty()
            // A retried message arrives under the same identifier: show it once.
            fresh = conversation.none { old -> old.id == message.id }
            if (!fresh) {
                it
            } else {
                val count = (it.unread[peer] ?: 0) + 1
                val unread = if (seen) it.unread else it.unread + (peer to count)
                it.copy(messages = it.messages + (peer to conversation + message), unread = unread)
            }
        }
        if (fresh && !seen) {
            val alias = _state.value.contacts.firstOrNull { it.id == peer }?.alias ?: "kontakt"
            notifier.show(peer, alias, _state.value.unread[peer] ?: 1)
        }
    }

    private fun markRead(id: String) {
        _state.update { it.copy(unread = it.unread - id) }
        notifier.cancel(id)
    }

    /**
     * Statuses that arrived before their message was stored: hashing a big
     * file happens before the message joins the list.
     */
    private val early = mutableMapOf<String, Status>()

    /** Moves our message forward, never back. */
    private fun setStatus(peer: String, id: String, status: Status) = _state.update { state ->
        val known = state.messages[peer]?.any { it.id == id } == true
        if (!known) {
            val previous = early[id]
            if (previous == null || previous.ordinal < status.ordinal) early[id] = status
            return@update state
        }
        val conversation = state.messages.getValue(peer)
        val updated = conversation.map { message ->
            if (message.id == id && message.status.ordinal < status.ordinal) {
                message.copy(status = status)
            } else {
                message
            }
        }
        state.copy(messages = state.messages + (peer to updated))
    }

    private fun append(peer: String, message: Message) = _state.update {
        val conversation = it.messages[peer].orEmpty()
        if (conversation.any { old -> old.id == message.id }) {
            it
        } else {
            it.copy(messages = it.messages + (peer to conversation + message))
        }
    }

    private fun withCore(action: (App) -> Unit) {
        val app = core ?: return
        scope.launch {
            try {
                action(app)
            } catch (e: Exception) {
                fail(e)
            }
        }
    }

    private fun fail(e: Exception) = _state.update {
        it.copy(error = e.message ?: e.toString())
    }
}
