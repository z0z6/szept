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

/** One line of a conversation. */
data class Message(val id: String, val mine: Boolean, val body: String)

data class UiState(
    val ready: Boolean = false,
    val myId: String = "",
    val contacts: List<Contact> = emptyList(),
    /** Identifiers of the contacts that are connected right now. */
    val online: Set<String> = emptySet(),
    /** Conversations, keyed by the contact's identifier. */
    val messages: Map<String, List<Message>> = emptyMap(),
    /** Identifiers of our messages confirmed by the other device. */
    val delivered: Set<String> = emptySet(),
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

    @Volatile
    private var core: App? = null

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
            is ChatEvent.Message -> append(event.peer, Message(event.id, false, event.body))
            is ChatEvent.Delivered -> _state.update { it.copy(delivered = it.delivered + event.id) }
        }
    }

    fun addContact(alias: String, id: String) = withCore { app ->
        app.addContact(alias, id)
        _state.update { it.copy(contacts = app.contacts()) }
    }

    fun removeContact(contact: Contact) = withCore { app ->
        app.removeContact(contact.alias)
        _state.update {
            it.copy(
                contacts = app.contacts(),
                online = it.online - contact.id,
                messages = it.messages - contact.id,
            )
        }
    }

    fun send(peer: String, body: String) = withCore { app ->
        val id = app.send(peer, body)
        append(peer, Message(id, true, body))
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

    private fun append(peer: String, message: Message) = _state.update {
        val conversation = it.messages[peer].orEmpty()
        // A retried message arrives under the same identifier: show it once.
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
