package org.komunikator.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
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
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Density
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.WindowCompat
import androidx.emoji2.emojipicker.EmojiPickerView
import android.view.inputmethod.InputMethodManager
import android.widget.Toast
import kotlinx.coroutines.delay
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import java.text.DateFormat
import java.util.Date
import uniffi.komunikator_ffi.Contact
import uniffi.komunikator_ffi.Group
import uniffi.komunikator_ffi.qrCode
import uniffi.komunikator_ffi.shareLink

class MainActivity : ComponentActivity() {
    private val model: Messenger by lazy { (application as SzeptApp).messenger }
    private val appearance: Appearance by lazy { (application as SzeptApp).appearance }

    // Without the permission Android 13+ hides every notification.
    private val askNotifications =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        SzeptService.start(this)
        openFrom(intent)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        setContent {
            val dark = appearance.isDark(isSystemInDarkTheme())
            val colors = colorSchemeOf(appearance.theme, dark)
            val view = LocalView.current
            SideEffect {
                // The system bars take the colours of the theme.
                @Suppress("DEPRECATION")
                window.statusBarColor = colors.surface.toArgb()
                @Suppress("DEPRECATION")
                window.navigationBarColor = colors.surface.toArgb()
                WindowCompat.getInsetsController(window, view).apply {
                    isAppearanceLightStatusBars = !dark
                    isAppearanceLightNavigationBars = !dark
                }
            }
            // The text size chosen in Szept, on top of the phone's own.
            val density = LocalDensity.current
            val scaled = Density(density.density, density.fontScale * appearance.textSize.scale)
            CompositionLocalProvider(LocalDensity provides scaled) {
                MaterialTheme(colorScheme = colors) {
                    Surface(modifier = Modifier.fillMaxSize()) {
                        Root(model, appearance, dark, onRestart = { SzeptService.start(this) })
                    }
                }
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
        VoicePlayback.stop()
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
private fun Root(model: Messenger, appearance: Appearance, dark: Boolean, onRestart: () -> Unit) {
    val state by model.state.collectAsState()
    val open = state.contacts.firstOrNull { it.id == state.openId }
    val openGroup = state.groups.firstOrNull { it.id == state.openId }

    BackHandler(enabled = open != null || openGroup != null) { model.open(null) }

    when {
        state.stopped -> Box(Modifier.fillMaxSize().padding(24.dp), Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text("Szept jest wyłączony", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Nie odbiera wiadomości ani plików, dopóki go nie włączysz.",
                    modifier = Modifier.padding(vertical = 12.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Button(onClick = onRestart) { Text("Włącz") }
            }
        }
        !state.ready && state.error == null -> Box(Modifier.fillMaxSize(), Alignment.Center) {
            CircularProgressIndicator()
        }
        open != null -> ChatScreen(
            key = open.id,
            title = open.alias,
            contact = open,
            group = null,
            online = open.id in state.online,
            picture = state.avatars[open.id],
            state = state,
            model = model,
            onBack = { model.open(null) },
        )
        openGroup != null -> ChatScreen(
            key = openGroup.id,
            title = openGroup.name,
            contact = null,
            group = openGroup,
            online = openGroup.members.any { it.id in state.online },
            picture = null,
            state = state,
            model = model,
            onBack = { model.open(null) },
        )
        else -> ContactsScreen(
            state = state,
            appearance = appearance,
            dark = dark,
            model = model,
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
    state.info?.let { message ->
        AlertDialog(
            onDismissRequest = model::clearInfo,
            confirmButton = { TextButton(onClick = model::clearInfo) { Text("OK") } },
            text = { Text(message) },
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
    "picture is too large" in message -> "To zdjęcie jest za duże."
    "JPEG, PNG, WebP or GIF" in message -> "Wybierz zdjęcie w formacie JPEG, PNG, WebP albo GIF."
    "media file is too large" in message -> "Ten plik jest za duży na wiadomość. Wyślij go spinaczem."
    "cannot be sent as media" in message -> "Tego pliku nie da się wysłać jako GIF."
    "wrong password, or the archive is damaged" in message -> "Złe hasło albo uszkodzone archiwum."
    "not a Szept archive" in message -> "To nie jest archiwum Szeptu."
    "newer Szept" in message -> "To archiwum pochodzi z nowszej wersji Szeptu. Najpierw zaktualizuj aplikację."
    "password must have at least" in message -> "Hasło musi mieć co najmniej 12 znaków."
    "too easy to guess" in message -> "To hasło łatwo odgadnąć. Użyj dłuższego, najlepiej z kilku słów."
    "unreasonable key settings" in message -> "To archiwum ma podejrzane ustawienia szyfrowania i nie zostanie otwarte."
    "already has this key" in message -> "Ten kontakt ma już ten klucz."
    "only contacts can be added" in message -> "Do grupy możesz dodać tylko swoje kontakty."
    "at least one other member" in message -> "Grupa potrzebuje co najmniej jednej osoby poza Tobą."
    "members" in message && "at most" in message -> "Grupa ma za dużo osób."
    "the group needs a name" in message -> "Nadaj grupie nazwę."
    "only the owner" in message -> "Grupę zmienia tylko jej właściciel."
    "not in this group" in message -> "Nie jesteś już w tej grupie."
    "name is too long" in message -> "To imię jest za długie."
    "too long" in message -> "Wiadomość jest za długa."
    else -> message
}

/** The last line of a conversation, short, for the list. */
private fun previewOf(last: Message): String {
    val file = last.file
    val text = when {
        last.system -> last.body
        last.sticker != null -> "Naklejka"
        file == null -> last.body
        file.media == "voice" -> "🎤 Wiadomość głosowa"
        file.media == "gif" -> "GIF"
        else -> "📎 ${file.name}"
    }
    return if (last.mine) "Ty: $text" else text
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ContactsScreen(
    state: UiState,
    appearance: Appearance,
    dark: Boolean,
    model: Messenger,
    onOpen: (Contact) -> Unit,
    onAdd: (String, String) -> Unit,
    onRemove: (Contact) -> Unit,
    parseId: (String) -> String?,
) {
    var adding by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf<Contact?>(null) }
    var rekeying by remember { mutableStateOf<Contact?>(null) }
    var menuFor by remember { mutableStateOf<String?>(null) }
    var newGroup by remember { mutableStateOf(false) }
    var settings by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Szept") },
                actions = {
                    TextButton(onClick = { newGroup = true }) { Text("Nowa grupa") }
                    IconButton(
                        onClick = { settings = true },
                        modifier = Modifier.semantics { contentDescription = "Ustawienia: Twoje zdjęcie i wygląd" },
                    ) { Avatar(alias = "", online = false, picture = state.myAvatar, size = 36) }
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(onClick = { adding = true }) { Text("Dodaj kontakt") }
        },
    ) { padding ->
        LazyColumn(modifier = Modifier.fillMaxSize().padding(padding)) {
            item { MyIdCard(state.myId) }
            item { BatteryCard() }
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
            if (state.groups.isNotEmpty()) {
                item {
                    Text(
                        "Grupy",
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                    )
                }
            }
            items(state.groups, key = { it.id }) { group ->
                val unread = state.unread[group.id] ?: 0
                val last = state.messages[group.id]?.lastOrNull()
                ListItem(
                    modifier = Modifier.clickable { model.open(group.id) },
                    leadingContent = {
                        Surface(
                            shape = RoundedCornerShape(12.dp),
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(40.dp),
                        ) {
                            Box(contentAlignment = Alignment.Center) {
                                Text(
                                    group.name.take(1).uppercase(),
                                    color = MaterialTheme.colorScheme.onPrimary,
                                    fontWeight = FontWeight.SemiBold,
                                )
                            }
                        }
                    },
                    headlineContent = {
                        Text(group.name, fontWeight = if (unread > 0) FontWeight.Bold else null)
                    },
                    supportingContent = {
                        Text(last?.let(::previewOf) ?: "${group.members.size} osób", maxLines = 1)
                    },
                    trailingContent = { if (unread > 0) Badge { Text("$unread") } },
                )
                HorizontalDivider()
            }
            if (state.contacts.isNotEmpty()) {
                item {
                    Text(
                        "Kontakty",
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                    )
                }
            }
            items(state.contacts, key = { it.id }) { contact ->
                val online = contact.id in state.online
                val unread = state.unread[contact.id] ?: 0
                val last = state.messages[contact.id]?.lastOrNull()
                ListItem(
                    modifier = Modifier.clickable { onOpen(contact) },
                    leadingContent = { Avatar(contact.alias, online, state.avatars[contact.id]) },
                    headlineContent = {
                        Text(contact.alias, fontWeight = if (unread > 0) FontWeight.Bold else null)
                    },
                    supportingContent = {
                        val preview = when {
                            last == null -> if (online) "Połączony" else "Niepołączony"
                            else -> previewOf(last)
                        }
                        Text(preview, maxLines = 1)
                    },
                    trailingContent = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            if (unread > 0) {
                                Badge { Text("$unread") }
                            }
                            Box {
                                TextButton(onClick = { menuFor = contact.id }) { Text("⋮") }
                                DropdownMenu(expanded = menuFor == contact.id, onDismissRequest = { menuFor = null }) {
                                    DropdownMenuItem(
                                        text = { Text("Nowy klucz kontaktu") },
                                        onClick = {
                                            menuFor = null
                                            rekeying = contact
                                        },
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Usuń kontakt") },
                                        onClick = {
                                            menuFor = null
                                            removing = contact
                                        },
                                    )
                                }
                            }
                        }
                    },
                )
                HorizontalDivider()
            }
            // Room for the floating button.
            item { Spacer(Modifier.height(88.dp)) }
        }
    }

    if (settings) {
        SettingsDialog(
            appearance = appearance,
            dark = dark,
            myAvatar = state.myAvatar,
            myName = state.myName,
            busy = state.busy,
            onPickAvatar = model::setAvatar,
            onRemoveAvatar = model::removeAvatar,
            onSaveName = model::setName,
            onExport = { uri, password, identity ->
                model.exportArchive(uri, password, identity, appearance.toPreferences())
            },
            onImport = { uri, password, identity ->
                model.restoreArchive(uri, password, identity, appearance::apply)
            },
            onDismiss = { settings = false },
        )
    }

    rekeying?.let { contact ->
        KeyUpdateDialog(
            contact = contact,
            parseId = parseId,
            onDismiss = { rekeying = null },
            onConfirm = { id ->
                rekeying = null
                model.updateContactKey(contact, id)
            },
        )
    }

    if (newGroup) {
        GroupDialog(
            title = "Nowa grupa",
            contacts = state.contacts,
            initialName = "",
            others = emptyList(),
            initialMembers = emptySet(),
            confirm = "Utwórz",
            onDismiss = { newGroup = false },
            onConfirm = { name, members ->
                newGroup = false
                model.createGroup(name, members)
            },
        )
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

/**
 * A contact's picture, or the first letter of their name when there is none.
 * Without a name (our own avatar), a person outline.
 */
@Composable
fun Avatar(alias: String, online: Boolean, picture: ImageBitmap? = null, size: Int = 40) {
    val colors = MaterialTheme.colorScheme
    Box(modifier = Modifier.size(size.dp)) {
        Box(
            modifier = Modifier.fillMaxSize().clip(CircleShape).background(colors.secondaryContainer),
            contentAlignment = Alignment.Center,
        ) {
            when {
                picture != null -> Image(
                    bitmap = picture,
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
                alias.isNotEmpty() -> Text(
                    alias.take(1).uppercase(),
                    color = colors.onSecondaryContainer,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = (size * 0.4f).sp,
                )
                else -> Canvas(modifier = Modifier.fillMaxSize(0.62f)) {
                    val c = this.size.minDimension
                    drawCircle(colors.primary, radius = c * 0.2f, center = Offset(c / 2, c * 0.33f))
                    drawArc(
                        color = colors.primary,
                        startAngle = 180f,
                        sweepAngle = 180f,
                        useCenter = true,
                        topLeft = Offset(c * 0.15f, c * 0.62f),
                        size = Size(c * 0.7f, c * 0.7f),
                    )
                }
            }
        }
        if (online) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .size((size * 0.3f).dp)
                    .clip(CircleShape)
                    .background(colors.surface)
                    .padding(2.dp)
                    .clip(CircleShape)
                    .background(colors.tertiary)
                    .semantics { contentDescription = "online" },
            )
        }
    }
}

/**
 * Many phones put apps to sleep despite the background service. Offers the
 * system switch that exempts Szept, until it is on or the person says no.
 */
@Composable
private fun BatteryCard() {
    val context = LocalContext.current
    val power = remember { context.getSystemService(PowerManager::class.java) }
    var exempt by remember { mutableStateOf(power.isIgnoringBatteryOptimizations(context.packageName)) }
    var dismissed by rememberSaveable { mutableStateOf(false) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        exempt = power.isIgnoringBatteryOptimizations(context.packageName)
    }
    if (exempt || dismissed) return
    Card(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text("Odbieranie w tle", style = MaterialTheme.typography.titleSmall)
            Text(
                "Żeby wiadomości i pliki docierały przy wygaszonym ekranie, " +
                    "pozwól Szeptowi działać bez oszczędzania baterii.",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(vertical = 6.dp),
            )
            Row {
                TextButton(onClick = {
                    val request = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                        .setData(Uri.parse("package:" + context.packageName))
                    try {
                        ask.launch(request)
                    } catch (e: Exception) {
                        ask.launch(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
                    }
                }) { Text("Pozwól") }
                TextButton(onClick = { dismissed = true }) { Text("Nie teraz") }
            }
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
    key: String,
    title: String,
    contact: Contact?,
    group: Group?,
    online: Boolean,
    picture: ImageBitmap?,
    state: UiState,
    model: Messenger,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val messages = state.messages[key].orEmpty()
    val people = peopleOf(state, group, contact?.alias, contact?.id)
    var draft by remember(key) { mutableStateOf("") }
    var editor by remember(key) { mutableStateOf<RichEditText?>(null) }
    val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) model.sendFiles(key, uris)
    }
    var picking by remember(key) { mutableStateOf(false) }
    var stickers by remember(key) { mutableStateOf(false) }
    var groupInfo by remember(key) { mutableStateOf(false) }
    val input = remember { context.getSystemService(InputMethodManager::class.java) }
    val listState = rememberLazyListState()
    val lastMine = messages.indexOfLast { it.mine }

    // Voice messages
    val recorder = remember { VoiceRecorder(context) }
    var recording by remember { mutableStateOf(false) }
    var elapsed by remember { mutableStateOf(0L) }
    fun startRecording() {
        try {
            recorder.start(model.newRecording())
            recording = true
            picking = false
            editor?.let { input.hideSoftInputFromWindow(it.windowToken, 0) }
        } catch (e: Exception) {
            Toast.makeText(context, "Nie udało się włączyć mikrofonu.", Toast.LENGTH_LONG).show()
        }
    }
    fun finishRecording(send: Boolean) {
        recording = false
        if (send) {
            recorder.finish()?.let { (file, length) -> model.sendVoice(key, file, length) }
        } else {
            recorder.cancel()
        }
    }
    val askMicrophone = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) {
            startRecording()
        } else {
            Toast.makeText(context, "Bez dostępu do mikrofonu nie nagrasz wiadomości.", Toast.LENGTH_LONG).show()
        }
    }
    LaunchedEffect(recording) {
        while (recording) {
            elapsed = recorder.elapsed()
            if (elapsed >= MAX_VOICE_MS) finishRecording(true)
            delay(200)
        }
    }
    // Leaving the conversation drops an unfinished recording.
    DisposableEffect(key) {
        onDispose { recorder.cancel() }
    }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.lastIndex)
    }
    BackHandler(enabled = picking || stickers || recording) {
        when {
            recording -> finishRecording(false)
            stickers -> stickers = false
            else -> picking = false
        }
    }

    if (groupInfo && group != null) {
        GroupInfoDialog(
            group = group,
            people = people,
            online = state.online,
            contacts = state.contacts,
            onDismiss = { groupInfo = false },
            onSave = { name, members ->
                groupInfo = false
                model.updateGroup(group.id, name, members)
            },
            onLeave = {
                groupInfo = false
                model.leaveGroup(group.id)
            },
        )
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = if (group != null) {
                            Modifier.clickable(onClickLabel = "Członkowie grupy") { groupInfo = true }
                        } else {
                            Modifier
                        },
                    ) {
                        Avatar(title, online, picture)
                        Spacer(Modifier.width(12.dp))
                        Column {
                            Text(title)
                            Text(
                                when {
                                    group != null -> {
                                        val connected = group.members.count { it.id in state.online }
                                        "${group.members.size} osób, połączonych: $connected · dotknij"
                                    }
                                    online -> "Połączony"
                                    else -> "Niepołączony, wiadomości poczekają"
                                },
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
                if (recording) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Box(
                            modifier = Modifier
                                .size(12.dp)
                                .clip(CircleShape)
                                .background(MaterialTheme.colorScheme.error),
                        )
                        Text(formatDuration(elapsed), fontWeight = FontWeight.SemiBold)
                        Text(
                            "Nagrywanie…",
                            modifier = Modifier.weight(1f),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        TextButton(onClick = { finishRecording(false) }) { Text("Anuluj") }
                        Button(onClick = { finishRecording(true) }) { Text("Wyślij") }
                    }
                } else {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        IconButton(
                            onClick = {
                                picking = !picking
                                stickers = false
                                editor?.let { field ->
                                    if (picking) {
                                        input.hideSoftInputFromWindow(field.windowToken, 0)
                                    } else {
                                        field.requestFocus()
                                        input.showSoftInput(field, 0)
                                    }
                                }
                            },
                            modifier = Modifier.semantics { contentDescription = "Emoji" },
                        ) { Text("😊", fontSize = 22.sp) }
                        IconButton(
                            onClick = {
                                stickers = !stickers
                                picking = false
                                editor?.let { input.hideSoftInputFromWindow(it.windowToken, 0) }
                            },
                            modifier = Modifier.semantics { contentDescription = "Naklejki" },
                        ) { FigureView("pierog", 28.dp, animate = false) }
                        IconButton(
                            onClick = { pickFiles.launch(arrayOf("*/*")) },
                            modifier = Modifier.semantics { contentDescription = "Wyślij pliki" },
                        ) { Text("📎", fontSize = 22.sp) }
                        MessageField(
                            onTextChange = { draft = it },
                            onPicture = { uri, mime ->
                                // Copied now: the keyboard's permission to read it is short-lived.
                                val staged = model.stageGif(uri, mime)
                                if (staged != null) {
                                    model.sendGif(key, staged)
                                } else {
                                    Toast.makeText(context, "Tego obrazu nie da się wysłać.", Toast.LENGTH_LONG).show()
                                }
                            },
                            onCreated = { editor = it },
                            modifier = Modifier.weight(1f),
                        )
                        if (draft.isBlank()) {
                            IconButton(
                                onClick = {
                                    val granted = context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) ==
                                        PackageManager.PERMISSION_GRANTED
                                    if (granted) startRecording() else askMicrophone.launch(Manifest.permission.RECORD_AUDIO)
                                },
                                modifier = Modifier.semantics { contentDescription = "Nagraj wiadomość głosową" },
                            ) { MicIcon() }
                        } else {
                            Button(onClick = {
                                model.send(key, draft.trim())
                                editor?.setText("")
                            }) { Text("Wyślij") }
                        }
                    }
                }
                if (stickers && !recording) {
                    StickerPanel { code ->
                        stickers = false
                        model.sendSticker(key, code)
                    }
                }
                if (picking && !recording) {
                    // The system's emoji set, with skin tones and recents.
                    AndroidView(
                        factory = { context ->
                            EmojiPickerView(context).apply {
                                setOnEmojiPickedListener { item ->
                                    editor?.let { field ->
                                        val start = field.selectionStart.coerceAtLeast(0)
                                        val end = field.selectionEnd.coerceAtLeast(0)
                                        field.text.replace(minOf(start, end), maxOf(start, end), item.emoji)
                                    }
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
                        "Napisz pierwszą wiadomość, nagraj głosówkę albo wyślij plik spinaczem. " +
                            "GIF-y wstawisz z klawiatury. Rozmowa trwa, dopóki Szept jest otwarty.",
                        modifier = Modifier.padding(24.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(messages.size, key = { messages[it].id }) { index ->
                val message = messages[index]
                MessageRow(
                    message = message,
                    showWord = index == lastMine,
                    inGroup = group != null,
                    people = people,
                    onReact = { reaction -> model.react(key, message.id, reaction) },
                ) {
                    val file = message.file!!
                    val from = message.author?.let(people::name) ?: title
                    if (file.media != null) {
                        MediaContent(file) { model.download(key, message.id) }
                    } else {
                        FileCard(file, message.mine, from) { model.download(key, message.id) }
                    }
                }
            }
        }
    }
}

/** The longest voice message, matching the desktop app. */
private const val MAX_VOICE_MS = 10 * 60 * 1000L

@Composable
private fun MicIcon() {
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    Canvas(modifier = Modifier.size(24.dp)) {
        val u = size.minDimension / 24f
        val stroke = Stroke(width = 1.8f * u, cap = StrokeCap.Round)
        drawRoundRect(
            color,
            topLeft = Offset(9f * u, 3f * u),
            size = Size(6f * u, 11f * u),
            cornerRadius = CornerRadius(3f * u),
            style = stroke,
        )
        drawArc(
            color,
            startAngle = 0f,
            sweepAngle = 180f,
            useCenter = false,
            topLeft = Offset(5.5f * u, 4.5f * u),
            size = Size(13f * u, 13f * u),
            style = stroke,
        )
        drawLine(color, Offset(12f * u, 17.5f * u), Offset(12f * u, 21f * u), strokeWidth = 1.8f * u, cap = StrokeCap.Round)
    }
}

private val timeFormat: DateFormat = DateFormat.getTimeInstance(DateFormat.SHORT)

private fun statusLabel(status: Status) = when (status) {
    Status.QUEUED -> "W kolejce"
    Status.SENT -> "Wysłano"
    Status.DELIVERED -> "Dostarczono"
    Status.RECEIVED -> ""
}

/** A clock, one tick or two ticks: queued, sent, delivered. */
@Composable
private fun StatusMark(status: Status, label: String) {
    val color = if (status == Status.DELIVERED) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant
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
