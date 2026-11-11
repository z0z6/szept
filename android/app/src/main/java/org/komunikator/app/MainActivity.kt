package org.komunikator.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import uniffi.komunikator_ffi.Contact

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val colors = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
            MaterialTheme(colorScheme = colors) {
                Surface(modifier = Modifier.fillMaxSize()) { Root() }
            }
        }
    }
}

@Composable
private fun Root(model: ChatViewModel = viewModel()) {
    val state by model.state.collectAsState()
    // Identifier of the contact whose conversation is open.
    var openId by rememberSaveable { mutableStateOf<String?>(null) }
    val open = state.contacts.firstOrNull { it.id == openId }

    BackHandler(enabled = open != null) { openId = null }

    when {
        !state.ready && state.error == null -> Box(Modifier.fillMaxSize(), Alignment.Center) {
            CircularProgressIndicator()
        }
        open != null -> ChatScreen(
            contact = open,
            online = open.id in state.online,
            messages = state.messages[open.id].orEmpty(),
            delivered = state.delivered,
            onSend = { model.send(open.id, it) },
            onBack = { openId = null },
        )
        else -> ContactsScreen(
            state = state,
            onOpen = { openId = it.id },
            onAdd = model::addContact,
            onRemove = model::removeContact,
        )
    }

    state.error?.let { message ->
        AlertDialog(
            onDismissRequest = model::clearError,
            confirmButton = { TextButton(onClick = model::clearError) { Text("OK") } },
            title = { Text("Coś poszło nie tak") },
            text = { Text(message) },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ContactsScreen(
    state: UiState,
    onOpen: (Contact) -> Unit,
    onAdd: (String, String) -> Unit,
    onRemove: (Contact) -> Unit,
) {
    var adding by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf<Contact?>(null) }

    Scaffold(
        topBar = { TopAppBar(title = { Text("Szept") }) },
        floatingActionButton = {
            ExtendedFloatingActionButton(onClick = { adding = true }) { Text("Dodaj kontakt") }
        },
    ) { padding ->
        LazyColumn(modifier = Modifier.fillMaxSize().padding(padding)) {
            item { MyIdCard(state.myId) }
            if (state.contacts.isEmpty()) {
                item {
                    Text(
                        "Nie masz jeszcze kontaktów. Wymień się identyfikatorami z drugą osobą.",
                        modifier = Modifier.padding(16.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(state.contacts, key = { it.id }) { contact ->
                val count = state.messages[contact.id].orEmpty().size
                ListItem(
                    modifier = Modifier.clickable { onOpen(contact) },
                    headlineContent = { Text(contact.alias) },
                    supportingContent = {
                        val status = if (contact.id in state.online) "online" else "offline"
                        Text(if (count > 0) "$status · wiadomości: $count" else status)
                    },
                    trailingContent = {
                        TextButton(onClick = { removing = contact }) { Text("Usuń") }
                    },
                )
                HorizontalDivider()
            }
        }
    }

    if (adding) {
        AddContactDialog(
            onDismiss = { adding = false },
            onConfirm = { alias, id ->
                adding = false
                onAdd(alias, id)
            },
        )
    }

    removing?.let { contact ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text("Usunąć kontakt ${contact.alias}?") },
            text = { Text("Rozmowa zniknie, a ta osoba nie będzie mogła się z Tobą połączyć.") },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    onRemove(contact)
                }) { Text("Usuń") }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Anuluj") } },
        )
    }
}

@Composable
private fun MyIdCard(myId: String) {
    val clipboard = LocalClipboardManager.current
    val context = LocalContext.current
    Card(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text("Twój identyfikator", style = MaterialTheme.typography.titleSmall)
            SelectionContainer {
                Text(
                    myId,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(vertical = 8.dp),
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = { clipboard.setText(AnnotatedString(myId)) }) {
                    Text("Kopiuj")
                }
                TextButton(onClick = {
                    val send = Intent(Intent.ACTION_SEND)
                        .setType("text/plain")
                        .putExtra(Intent.EXTRA_TEXT, myId)
                    context.startActivity(Intent.createChooser(send, null))
                }) {
                    Text("Udostępnij")
                }
            }
        }
    }
}

@Composable
private fun AddContactDialog(onDismiss: () -> Unit, onConfirm: (String, String) -> Unit) {
    var alias by remember { mutableStateOf("") }
    var id by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Nowy kontakt") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = alias,
                    onValueChange = { alias = it },
                    label = { Text("Nazwa (jedno słowo)") },
                    singleLine = true,
                )
                OutlinedTextField(
                    value = id,
                    onValueChange = { id = it },
                    label = { Text("Identyfikator") },
                    minLines = 3,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(alias.trim(), id.trim()) },
                enabled = alias.isNotBlank() && id.isNotBlank(),
            ) { Text("Dodaj") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Anuluj") } },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ChatScreen(
    contact: Contact,
    online: Boolean,
    messages: List<Message>,
    delivered: Set<String>,
    onSend: (String) -> Unit,
    onBack: () -> Unit,
) {
    var draft by rememberSaveable(contact.id) { mutableStateOf("") }
    val listState = rememberLazyListState()

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.lastIndex)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(contact.alias)
                        Text(
                            if (online) "online" else "offline, wiadomości poczekają w kolejce",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                },
                navigationIcon = { TextButton(onClick = onBack) { Text("Wróć") } },
            )
        },
        bottomBar = {
            Row(
                modifier = Modifier.fillMaxWidth().padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("Wiadomość") },
                    maxLines = 4,
                )
                Button(
                    onClick = {
                        onSend(draft.trim())
                        draft = ""
                    },
                    enabled = draft.isNotBlank(),
                ) { Text("Wyślij") }
            }
        },
    ) { padding ->
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize().padding(padding).padding(horizontal = 12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            items(messages, key = { it.id }) { message ->
                Bubble(message, sent = message.id in delivered)
            }
        }
    }
}

@Composable
private fun Bubble(message: Message, sent: Boolean) {
    val colors = MaterialTheme.colorScheme
    Box(
        modifier = Modifier.fillMaxWidth(),
        contentAlignment = if (message.mine) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Surface(
            color = if (message.mine) colors.primaryContainer else colors.surfaceVariant,
            shape = RoundedCornerShape(16.dp),
            modifier = Modifier.widthIn(max = 300.dp),
        ) {
            Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                Text(message.body)
                if (message.mine) {
                    Text(
                        if (sent) "dostarczono" else "czeka",
                        style = MaterialTheme.typography.labelSmall,
                        color = colors.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
