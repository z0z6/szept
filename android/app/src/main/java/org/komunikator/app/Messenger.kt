package org.komunikator.app

import android.app.Application
import android.net.Uri
import android.provider.OpenableColumns
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
import uniffi.komunikator_ffi.fileKind
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
                    _state.update {
                        it.copy(ready = true, stopped = false, myId = app.myId(), contacts = app.contacts())
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
            is ChatEvent.FileOffered -> receive(
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
                        transfer = Transfer.OFFERED,
                    ),
                ),
            )
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

    /** Starts or resumes fetching a file the contact offered. */
    fun download(peer: String, id: String) = withCore { app ->
        app.download(id, incomingDir.absolutePath)
        updateFile(peer, id) { it.copy(transfer = Transfer.DOWNLOADING, reason = null) }
    }

    /** Checks scanned or pasted text. Returns the identifier, or null. */
    fun parseId(text: String): String? = try {
        normalizeId(text)
    } catch (e: Exception) {
        null
    }

    fun clearError() = _state.update { it.copy(error = null) }

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
