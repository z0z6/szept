package org.komunikator.app

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlin.concurrent.thread
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.komunikator_ffi.App
import uniffi.komunikator_ffi.ChatEvent
import uniffi.komunikator_ffi.Contact
import uniffi.komunikator_ffi.EventListener
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

/** One line of a conversation. */
data class Message(
    val id: String,
    val mine: Boolean,
    val body: String,
    val atMs: Long,
    val status: Status,
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
)

/**
 * Owns the Rust core and turns its events into screen state.
 *
 * Conversations live in memory only: they are gone when Android stops the
 * process. Persistent history arrives with the encrypted database.
 */
class ChatViewModel(application: Application) : AndroidViewModel(application), EventListener {
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()

    private val notifier = Notifier(application)

    /** Orders "sent" and "delivered" after the message they are about. */
    private val sendLock = Any()

    @Volatile
    private var core: App? = null

    @Volatile
    private var foreground = false

    init {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val app = App(application.filesDir.absolutePath, false)
                app.start(this@ChatViewModel)
                core = app
                _state.update {
                    it.copy(ready = true, myId = app.myId(), contacts = app.contacts())
                }
            } catch (e: Exception) {
                fail(e)
            }
        }
    }

    /** Called by the Rust core on one of its own threads. */
    override fun onEvent(event: ChatEvent) {
        when (event) {
            is ChatEvent.Connected -> _state.update { it.copy(online = it.online + event.peer) }
            is ChatEvent.Disconnected -> _state.update { it.copy(online = it.online - event.peer) }
            is ChatEvent.Message -> receive(event)
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

    /** Checks scanned or pasted text. Returns the identifier, or null. */
    fun parseId(text: String): String? = try {
        normalizeId(text)
    } catch (e: Exception) {
        null
    }

    fun clearError() = _state.update { it.copy(error = null) }

    override fun onCleared() {
        val app = core ?: return
        core = null
        // Shutting the network down blocks for a moment.
        thread {
            app.stop()
            app.destroy()
        }
    }

    private fun receive(event: ChatEvent.Message) {
        val message = Message(event.id, false, event.body, event.sentAtMs.toLong(), Status.RECEIVED)
        val seen = foreground && _state.value.openId == event.peer
        var fresh = false
        _state.update {
            val conversation = it.messages[event.peer].orEmpty()
            // A retried message arrives under the same identifier: show it once.
            fresh = conversation.none { old -> old.id == message.id }
            if (!fresh) {
                it
            } else {
                val count = (it.unread[event.peer] ?: 0) + 1
                val unread = if (seen) it.unread else it.unread + (event.peer to count)
                it.copy(messages = it.messages + (event.peer to conversation + message), unread = unread)
            }
        }
        if (fresh && !seen) {
            val alias = _state.value.contacts.firstOrNull { it.id == event.peer }?.alias ?: "kontakt"
            notifier.show(event.peer, alias, _state.value.unread[event.peer] ?: 1)
        }
    }

    private fun markRead(id: String) {
        _state.update { it.copy(unread = it.unread - id) }
        notifier.cancel(id)
    }

    /** Moves our message forward, never back. */
    private fun setStatus(peer: String, id: String, status: Status) = _state.update { state ->
        val conversation = state.messages[peer] ?: return@update state
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
        viewModelScope.launch(Dispatchers.IO) {
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
