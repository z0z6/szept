package org.komunikator.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.emoji2.emojipicker.EmojiPickerView
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import java.text.DateFormat
import java.util.Date
import uniffi.komunikator_ffi.Contact
import uniffi.komunikator_ffi.qrCode
import uniffi.komunikator_ffi.shareLink

class MainActivity : ComponentActivity() {
    private val model: ChatViewModel by viewModels()

    // Without the permission Android 13+ hides every notification.
    private val askNotifications =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        openFrom(intent)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        setContent {
            val colors = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
            MaterialTheme(colorScheme = colors) {
                Surface(modifier = Modifier.fillMaxSize()) { Root(model) }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openFrom(intent)
    }

    override fun onStart() {
        super.onStart()
        model.setForeground(true)
    }

    override fun onStop() {
        model.setForeground(false)
        super.onStop()
    }

    /** A tap on a notification opens that conversation. */
    private fun openFrom(intent: Intent?) {
        if (intent?.action == Notifier.ACTION_OPEN) {
            intent.getStringExtra(Notifier.EXTRA_PEER)?.let(model::open)
        }
    }
}

@Composable
private fun Root(model: ChatViewModel) {
    val state by model.state.collectAsState()
    val open = state.contacts.firstOrNull { it.id == state.openId }

    BackHandler(enabled = open != null) { model.open(null) }

    when {
        !state.ready && state.error == null -> Box(Modifier.fillMaxSize(), Alignment.Center) {
            CircularProgressIndicator()
        }
        open != null -> ChatScreen(
            contact = open,
            online = open.id in state.online,
            messages = state.messages[open.id].orEmpty(),
            onSend = { model.send(open.id, it) },
            onBack = { model.open(null) },
        )
        else -> ContactsScreen(
            state = state,
            onOpen = { model.open(it.id) },
            onAdd = model::addContact,
            onRemove = model::removeContact,
            parseId = model::parseId,
        )
    }

    state.error?.let { message ->
        AlertDialog(
            onDismissRequest = model::clearError,
            confirmButton = { TextButton(onClick = model::clearError) { Text("OK") } },
            title = { Text("Coś poszło nie tak") },
            text = { Text(friendly(message)) },
        )
    }
}

/** The core reports errors in English: translate the ones people meet. */
private fun friendly(message: String): String = when {
    "alias must be a single word" in message -> "Nazwa musi być jednym słowem, bez spacji."
    "is already taken" in message -> "Masz już kontakt o tej nazwie."
    "already saved as" in message -> "Ta osoba jest już w kontaktach."
    "your own identifier" in message -> "To Twój własny identyfikator."
    "not a valid identifier" in message || "identifier is empty" in message ->
        "To nie jest identyfikator Szeptu."
    "too long" in message -> "Wiadomość jest za długa."
    else -> message
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ContactsScreen(
    state: UiState,
    onOpen: (Contact) -> Unit,
    onAdd: (String, String) -> Unit,
    onRemove: (Contact) -> Unit,
    parseId: (String) -> String?,
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
                        "Nie masz jeszcze kontaktów. Zeskanuj kod drugiej osoby przyciskiem " +
                            "Dodaj kontakt i pokaż jej swój, żeby mogła dodać Ciebie.",
                        modifier = Modifier.padding(16.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(state.contacts, key = { it.id }) { contact ->
                val online = contact.id in state.online
                val unread = state.unread[contact.id] ?: 0
                val last = state.messages[contact.id]?.lastOrNull()
                ListItem(
                    modifier = Modifier.clickable { onOpen(contact) },
                    leadingContent = { Avatar(contact.alias, online) },
                    headlineContent = {
                        Text(contact.alias, fontWeight = if (unread > 0) FontWeight.Bold else null)
                    },
                    supportingContent = {
                        val preview = when {
                            last == null -> if (online) "Połączony" else "Niepołączony"
                            last.mine -> "Ty: ${last.body}"
                            else -> last.body
                        }
                        Text(preview, maxLines = 1)
                    },
                    trailingContent = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            if (unread > 0) {
                                Badge { Text("$unread") }
                            }
                            TextButton(onClick = { removing = contact }) { Text("Usuń") }
                        }
                    },
                )
                HorizontalDivider()
            }
            // Room for the floating button.
            item { Spacer(Modifier.height(88.dp)) }
        }
    }

    if (adding) {
        AddContactDialog(
            parseId = parseId,
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
private fun Avatar(alias: String, online: Boolean) {
    val colors = MaterialTheme.colorScheme
    Box(modifier = Modifier.size(40.dp)) {
        Box(
            modifier = Modifier.fillMaxSize().clip(CircleShape).background(colors.secondaryContainer),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                alias.take(1).uppercase(),
                color = colors.onSecondaryContainer,
                fontWeight = FontWeight.SemiBold,
            )
        }
        if (online) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .size(12.dp)
                    .clip(CircleShape)
                    .background(colors.surface)
                    .padding(2.dp)
                    .clip(CircleShape)
                    .background(Color(0xFF2E7D6B))
                    .semantics { contentDescription = "online" },
            )
        }
    }
}

@Composable
private fun MyIdCard(myId: String) {
    val clipboard = LocalClipboardManager.current
    val context = LocalContext.current
    var enlarged by remember { mutableStateOf(false) }
    val link = remember(myId) { shareLink(myId) }

    Card(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        shape = RoundedCornerShape(24.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
        ),
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            QrImage(
                text = link,
                modifier = Modifier
                    .size(112.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .clickable { enlarged = true }
                    .semantics { contentDescription = "Twój kod QR. Dotknij, aby powiększyć." },
            )
            Column {
                Text("Twój kod", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Pokaż go drugiej osobie albo prześlij jej identyfikator.",
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(vertical = 4.dp),
                )
                Row {
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

    if (enlarged) {
        AlertDialog(
            onDismissRequest = { enlarged = false },
            confirmButton = { TextButton(onClick = { enlarged = false }) { Text("Zamknij") } },
            title = { Text("Twój kod") },
            text = {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    QrImage(
                        text = link,
                        modifier = Modifier.size(260.dp).clip(RoundedCornerShape(12.dp)),
                    )
                    SelectionContainer {
                        Text(
                            myId,
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(top = 12.dp),
                        )
                    }
                }
            },
        )
    }
}

/** Draws a QR code made by the Rust core, with the quiet zone around it. */
@Composable
private fun QrImage(text: String, modifier: Modifier = Modifier) {
    val code = remember(text) { runCatching { qrCode(text) }.getOrNull() } ?: return
    val size = code.size.toInt()
    Canvas(modifier = modifier.background(Color.White)) {
        val cell = this.size.minDimension / (size + 8)
        // A hair of overlap hides seams between neighbouring modules.
        val module = Size(cell + 0.5f, cell + 0.5f)
        code.modules.forEachIndexed { index, dark ->
            if (dark) {
                drawRect(
                    color = Color(0xFF1B2140),
                    topLeft = Offset((index % size + 4) * cell, (index / size + 4) * cell),
                    size = module,
                )
            }
        }
    }
}

@Composable
private fun AddContactDialog(
    parseId: (String) -> String?,
    onDismiss: () -> Unit,
    onConfirm: (String, String) -> Unit,
) {
    val context = LocalContext.current
    var alias by rememberSaveable { mutableStateOf("") }
    var id by rememberSaveable { mutableStateOf("") }
    var note by rememberSaveable { mutableStateOf<String?>(null) }
    val hasCamera = remember {
        context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)
    }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
        val scanned = result.contents ?: return@rememberLauncherForActivityResult
        val parsed = parseId(scanned)
        if (parsed != null) {
            id = parsed
            note = "Kod odczytany. Nadaj kontaktowi nazwę."
        } else {
            note = "To nie jest kod Szeptu. Zeskanuj kod z aplikacji drugiej osoby."
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Nowy kontakt") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                if (hasCamera) {
                    OutlinedButton(
                        onClick = {
                            scanner.launch(
                                ScanOptions()
                                    .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                                    .setPrompt("Skieruj aparat na kod QR drugiej osoby")
                                    .setBeepEnabled(false)
                                    .setOrientationLocked(false),
                            )
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) { Text("Skanuj kod QR") }
                }
                note?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall)
                }
                OutlinedTextField(
                    value = id,
                    onValueChange = { id = it },
                    label = { Text("Identyfikator") },
                    minLines = 3,
                )
                OutlinedTextField(
                    value = alias,
                    onValueChange = { alias = it },
                    label = { Text("Nazwa (jedno słowo)") },
                    singleLine = true,
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
    onSend: (String) -> Unit,
    onBack: () -> Unit,
) {
    var draft by remember(contact.id) { mutableStateOf(TextFieldValue("")) }
    var picking by remember(contact.id) { mutableStateOf(false) }
    val keyboard = LocalSoftwareKeyboardController.current
    val listState = rememberLazyListState()
    val lastMine = messages.indexOfLast { it.mine }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.lastIndex)
    }
    BackHandler(enabled = picking) { picking = false }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Avatar(contact.alias, online)
                        Spacer(Modifier.width(12.dp))
                        Column {
                            Text(contact.alias)
                            Text(
                                if (online) "Połączony" else "Niepołączony, wiadomości poczekają",
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                    }
                },
                navigationIcon = { TextButton(onClick = onBack) { Text("Wróć") } },
            )
        },
        bottomBar = {
            Column {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    IconButton(
                        onClick = {
                            picking = !picking
                            if (picking) keyboard?.hide() else keyboard?.show()
                        },
                        modifier = Modifier.semantics { contentDescription = "Emoji" },
                    ) { Text("😊", fontSize = 22.sp) }
                    OutlinedTextField(
                        value = draft,
                        onValueChange = { draft = it },
                        modifier = Modifier.weight(1f),
                        placeholder = { Text("Wiadomość") },
                        maxLines = 4,
                    )
                    Button(
                        onClick = {
                            onSend(draft.text.trim())
                            draft = TextFieldValue("")
                        },
                        enabled = draft.text.isNotBlank(),
                    ) { Text("Wyślij") }
                }
                if (picking) {
                    // The system's emoji set, with skin tones and recents.
                    AndroidView(
                        factory = { context ->
                            EmojiPickerView(context).apply {
                                setOnEmojiPickedListener { item ->
                                    val text = draft.text
                                    val start = draft.selection.min
                                    val end = draft.selection.max
                                    val updated = text.substring(0, start) + item.emoji + text.substring(end)
                                    val cursor = start + item.emoji.length
                                    draft = TextFieldValue(updated, TextRange(cursor))
                                }
                            }
                        },
                        modifier = Modifier.fillMaxWidth().height(280.dp),
                    )
                }
            }
        },
    ) { padding ->
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize().padding(padding).padding(horizontal = 12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            if (messages.isEmpty()) {
                item {
                    Text(
                        "Napisz pierwszą wiadomość. Rozmowa trwa, dopóki Szept jest otwarty.",
                        modifier = Modifier.padding(24.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(messages.size, key = { messages[it].id }) { index ->
                Bubble(messages[index], showWord = index == lastMine)
            }
        }
    }
}

private val timeFormat: DateFormat = DateFormat.getTimeInstance(DateFormat.SHORT)

@Composable
private fun Bubble(message: Message, showWord: Boolean) {
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
                Row(
                    modifier = Modifier.align(Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    val label = statusLabel(message.status)
                    if (message.mine && showWord) {
                        Text(label, style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
                    }
                    Text(
                        timeFormat.format(Date(message.atMs)),
                        style = MaterialTheme.typography.labelSmall,
                        color = colors.onSurfaceVariant,
                    )
                    if (message.mine) StatusMark(message.status, label)
                }
            }
        }
    }
}

private fun statusLabel(status: Status) = when (status) {
    Status.QUEUED -> "W kolejce"
    Status.SENT -> "Wysłano"
    Status.DELIVERED -> "Dostarczono"
    Status.RECEIVED -> ""
}

/** A clock, one tick or two ticks: queued, sent, delivered. */
@Composable
private fun StatusMark(status: Status, label: String) {
    val color = if (status == Status.DELIVERED) Color(0xFF2E7D6B) else MaterialTheme.colorScheme.onSurfaceVariant
    Canvas(modifier = Modifier.size(width = 16.dp, height = 12.dp).semantics { contentDescription = label }) {
        val unit = size.width / 16f
        val stroke = Stroke(width = 1.5f * unit, cap = StrokeCap.Round, join = StrokeJoin.Round)
        fun tick(dx: Float) = Path().apply {
            moveTo((2f + dx) * unit, 6.4f * unit)
            lineTo((5f + dx) * unit, 9.2f * unit)
            lineTo((11.6f + dx) * unit, 2.8f * unit)
        }
        when (status) {
            Status.QUEUED -> {
                drawCircle(color, radius = 4.6f * unit, center = Offset(8f * unit, 6f * unit), style = stroke)
                drawPath(
                    Path().apply {
                        moveTo(8f * unit, 3.6f * unit)
                        lineTo(8f * unit, 6f * unit)
                        lineTo(9.6f * unit, 7.2f * unit)
                    },
                    color,
                    style = stroke,
                )
            }
            Status.SENT -> drawPath(tick(1f), color, style = stroke)
            Status.DELIVERED -> {
                drawPath(tick(-1f), color, style = stroke)
                drawPath(tick(3f), color, style = stroke)
            }
            Status.RECEIVED -> {}
        }
    }
}
