package org.komunikator.app

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import java.text.DateFormat
import java.util.Date
import uniffi.komunikator_ffi.Contact
import uniffi.komunikator_ffi.Group
import uniffi.komunikator_ffi.passwordStrength
import uniffi.komunikator_ffi.suggestPassphrase

private val clock: DateFormat = DateFormat.getTimeInstance(DateFormat.SHORT)

/** How a conversation names the people in it. */
class People(
    val myId: String,
    private val names: Map<String, String>,
    val pictures: Map<String, ImageBitmap>,
) {
    fun name(id: String): String = if (id == myId) "Ty" else names[id] ?: "Ktoś spoza grupy"
}

/** Builds the names for a conversation: our aliases first, then names people chose, then the owner's list. */
fun peopleOf(state: UiState, group: Group?, contactAlias: String?, contactId: String?): People {
    val names = mutableMapOf<String, String>()
    group?.members?.forEach { member -> if (member.name.isNotEmpty()) names[member.id] = member.name }
    names.putAll(state.names)
    state.contacts.forEach { names[it.id] = it.alias }
    if (contactId != null && contactAlias != null) names[contactId] = contactAlias
    return People(state.myId, names, state.avatars)
}

/**
 * One message: in a group with the author's picture and name, below it the
 * reactions. A long press opens the palette of Szeptusie.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun MessageRow(
    message: Message,
    showWord: Boolean,
    inGroup: Boolean,
    people: People,
    onReact: (String) -> Unit,
    fileContent: @Composable () -> Unit,
) {
    if (message.system) {
        Box(Modifier.fillMaxWidth().padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
            Surface(
                shape = RoundedCornerShape(12.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                color = MaterialTheme.colorScheme.surface,
            ) {
                Text(
                    message.body,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
        }
        return
    }
    val colors = MaterialTheme.colorScheme
    var palette by remember { mutableStateOf(false) }
    val mine = message.reactions[people.myId]
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (message.mine) Arrangement.End else Arrangement.Start,
        verticalAlignment = Alignment.Bottom,
    ) {
        if (inGroup && !message.mine) {
            val author = message.author.orEmpty()
            Box(Modifier.padding(end = 6.dp, bottom = 22.dp)) {
                Avatar(people.name(author), online = false, picture = people.pictures[author], size = 28)
            }
        }
        Column(horizontalAlignment = if (message.mine) Alignment.End else Alignment.Start) {
            val sticker = message.sticker
            val bubbleColor = when {
                sticker != null -> colors.surface.copy(alpha = 0f)
                message.mine -> colors.primaryContainer
                else -> colors.surfaceContainerHighest
            }
            Box {
                Surface(
                    color = bubbleColor,
                    shape = RoundedCornerShape(16.dp),
                    modifier = Modifier
                        .widthIn(max = 300.dp)
                        .combinedClickable(
                            onClick = {},
                            onLongClick = { palette = true },
                            onLongClickLabel = "Zareaguj",
                        ),
                ) {
                    Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                        if (inGroup && !message.mine && message.author != null) {
                            Text(
                                people.name(message.author),
                                style = MaterialTheme.typography.labelMedium,
                                fontWeight = FontWeight.Bold,
                                color = colors.primary,
                            )
                        }
                        when {
                            sticker != null -> StickerContent(sticker)
                            message.file != null -> fileContent()
                            else -> Text(message.body)
                        }
                        Meta(message, showWord)
                    }
                }
                if (palette) {
                    ReactionPalette(
                        current = mine,
                        onPick = { code ->
                            palette = false
                            onReact(if (code == mine) "" else code)
                        },
                        onDismiss = { palette = false },
                    )
                }
            }
            Reactions(message, people, onToggle = { code -> onReact(if (code == mine) "" else code) })
        }
    }
}

@Composable
private fun Meta(message: Message, showWord: Boolean) {
    val colors = MaterialTheme.colorScheme
    Row(
        modifier = Modifier.padding(top = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        val label = statusWord(message.status)
        if (message.mine && showWord) {
            Text(label, style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
        }
        Text(
            clock.format(Date(message.atMs)),
            style = MaterialTheme.typography.labelSmall,
            color = colors.onSurfaceVariant,
        )
        if (message.mine) {
            Text(
                when (message.status) {
                    Status.QUEUED -> "🕓"
                    Status.SENT -> "✓"
                    Status.DELIVERED, Status.RECEIVED -> "✓✓"
                },
                style = MaterialTheme.typography.labelSmall,
                color = if (message.status == Status.DELIVERED) colors.tertiary else colors.onSurfaceVariant,
                modifier = Modifier.semantics { contentDescription = label },
            )
        }
    }
}

private fun statusWord(status: Status) = when (status) {
    Status.QUEUED -> "Czeka na kontakt"
    Status.SENT -> "Wysłano"
    Status.DELIVERED -> "Dostarczono"
    Status.RECEIVED -> "Odebrano"
}

/** One chip per kind of reaction, with how many people chose it. */
@Composable
private fun Reactions(message: Message, people: People, onToggle: (String) -> Unit) {
    if (message.reactions.isEmpty()) return
    val colors = MaterialTheme.colorScheme
    val grouped = message.reactions.entries
        .filter { FIGURES.containsKey(it.value) }
        .groupBy({ it.value }, { it.key })
    Row(
        modifier = Modifier.padding(top = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        grouped.forEach { (code, who) ->
            val own = people.myId in who
            val names = who.joinToString(", ") { people.name(it) }
            Surface(
                shape = RoundedCornerShape(50),
                color = if (own) colors.primaryContainer else colors.surfaceContainerHighest,
                border = BorderStroke(1.dp, if (own) colors.primary else colors.outlineVariant),
                modifier = Modifier
                    .clickable(onClickLabel = if (own) "Cofnij reakcję" else "Dołącz") { onToggle(code) }
                    .semantics { contentDescription = "${FIGURES[code]?.name}: $names" },
            ) {
                Row(
                    modifier = Modifier.padding(start = 3.dp, end = 8.dp, top = 1.dp, bottom = 1.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    FigureView(code, 24.dp, animate = false)
                    if (who.size > 1) Text("${who.size}", style = MaterialTheme.typography.labelMedium)
                }
            }
        }
    }
}

/** The palette of Szeptusie above a message, all of them moving. */
@Composable
private fun ReactionPalette(current: String?, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Popup(
        alignment = Alignment.TopCenter,
        offset = androidx.compose.ui.unit.IntOffset(0, -170),
        onDismissRequest = onDismiss,
        properties = PopupProperties(focusable = true),
    ) {
        Surface(
            shape = RoundedCornerShape(20.dp),
            color = colors.surfaceContainerHighest,
            shadowElevation = 8.dp,
            tonalElevation = 2.dp,
        ) {
            Column(modifier = Modifier.padding(8.dp)) {
                REACTIONS.chunked(5).forEach { row ->
                    Row {
                        row.forEach { code ->
                            val figure = FIGURES.getValue(code)
                            Surface(
                                shape = RoundedCornerShape(12.dp),
                                color = if (code == current) colors.primaryContainer else colors.surfaceContainerHighest,
                                modifier = Modifier
                                    .padding(2.dp)
                                    .clickable(onClickLabel = figure.name) { onPick(code) },
                            ) {
                                Box(Modifier.padding(4.dp)) { FigureView(code, 40.dp, label = figure.name) }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun StickerContent(code: String) {
    val figure = FIGURES[code]
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        FigureView(code, 128.dp, label = figure?.caption?.let { "Naklejka: $it" })
        Text(
            figure?.caption.orEmpty(),
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = 150.dp),
        )
    }
}

/** The sticker drawer under the message field. */
@Composable
fun StickerPanel(onPick: (String) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp)) {
        Text(
            "Szeptusie: wyślij naklejkę",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(4.dp),
        )
        LazyVerticalGrid(
            columns = GridCells.Adaptive(88.dp),
            modifier = Modifier.fillMaxWidth().heightIn(max = 300.dp),
        ) {
            items(STICKERS) { code ->
                val figure = FIGURES.getValue(code)
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    modifier = Modifier
                        .padding(4.dp)
                        .clickable(onClickLabel = "Wyślij naklejkę: ${figure.caption}") { onPick(code) }
                        .padding(4.dp),
                ) {
                    FigureView(code, 64.dp)
                    Text(
                        figure.caption,
                        style = MaterialTheme.typography.labelSmall,
                        textAlign = TextAlign.Center,
                        maxLines = 2,
                    )
                }
            }
        }
    }
}

/** Picks a group's name and members: for a new group, or one we own. */
@Composable
fun GroupDialog(
    title: String,
    contacts: List<Contact>,
    initialName: String,
    /** People already in it who are not our contacts, with their names. */
    others: List<Pair<String, String>>,
    initialMembers: Set<String>,
    confirm: String,
    onDismiss: () -> Unit,
    onConfirm: (String, List<String>) -> Unit,
) {
    var name by remember { mutableStateOf(initialName) }
    val chosen = remember { mutableStateListOf<String>().apply { addAll(initialMembers) } }
    var error by remember { mutableStateOf<String?>(null) }
    val rows = contacts.map { it.id to it.alias } + others.filter { (id, _) -> contacts.none { it.id == id } }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(64) },
                    label = { Text("Nazwa grupy") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.size(8.dp))
                Text("Członkowie", style = MaterialTheme.typography.titleSmall)
                rows.forEach { (id, label) ->
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { if (id in chosen) chosen.remove(id) else chosen.add(id) },
                    ) {
                        Checkbox(checked = id in chosen, onCheckedChange = null)
                        Spacer(Modifier.width(8.dp))
                        Text(label)
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                when {
                    name.isBlank() -> error = "Nadaj grupie nazwę."
                    chosen.isEmpty() -> error = "Zaznacz co najmniej jedną osobę."
                    else -> onConfirm(name.trim(), chosen.toList())
                }
            }) { Text(confirm) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Anuluj") } },
    )
}

/** Who is in a group; the owner can change it, everyone can leave. */
@Composable
fun GroupInfoDialog(
    group: Group,
    people: People,
    online: Set<String>,
    contacts: List<Contact>,
    onDismiss: () -> Unit,
    onSave: (String, List<String>) -> Unit,
    onLeave: () -> Unit,
) {
    var editing by remember { mutableStateOf(false) }
    var leaving by remember { mutableStateOf(false) }
    if (editing) {
        GroupDialog(
            title = "Zmień grupę",
            contacts = contacts,
            initialName = group.name,
            others = group.members.filter { it.id != people.myId }.map { it.id to people.name(it.id) },
            initialMembers = group.members.map { it.id }.filter { it != people.myId }.toSet(),
            confirm = "Zapisz",
            onDismiss = { editing = false },
            onConfirm = { name, members ->
                editing = false
                onSave(name, members)
            },
        )
        return
    }
    if (leaving) {
        AlertDialog(
            onDismissRequest = { leaving = false },
            title = { Text(if (group.mine) "Zamknąć grupę?" else "Opuścić grupę?") },
            text = {
                Text(
                    if (group.mine) {
                        "Grupa zniknie u wszystkich członków."
                    } else {
                        "Rozmowa zniknie z tego telefonu."
                    },
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    leaving = false
                    onLeave()
                }) { Text(if (group.mine) "Zamknij" else "Opuść") }
            },
            dismissButton = { TextButton(onClick = { leaving = false }) { Text("Anuluj") } },
        )
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(group.name) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                group.members.forEach { member ->
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier.padding(vertical = 4.dp),
                    ) {
                        Avatar(people.name(member.id), member.id in online, people.pictures[member.id], size = 32)
                        Spacer(Modifier.width(10.dp))
                        Text(
                            people.name(member.id) + if (member.id == group.owner) " · właściciel" else "",
                        )
                    }
                }
                if (!group.mine) {
                    Text(
                        "Członków zmienia właściciel grupy.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        confirmButton = {
            Row {
                TextButton(onClick = { leaving = true }) {
                    Text(if (group.mine) "Zamknij grupę" else "Opuść", color = MaterialTheme.colorScheme.error)
                }
                if (group.mine) TextButton(onClick = { editing = true }) { Text("Zmień") }
                TextButton(onClick = onDismiss) { Text("Gotowe") }
            }
        },
    )
}

/** Gives a contact the identifier they got after reinstalling Szept. */
@Composable
fun KeyUpdateDialog(
    contact: Contact,
    parseId: (String) -> String?,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var text by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Nowy klucz: ${contact.alias}") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(
                    "Użyj tego, gdy ta osoba zainstalowała Szept od nowa i ma nowy identyfikator. " +
                        "Stara rozmowa zostanie zakończona, a nowa będzie toczyć się pod nią.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.size(8.dp))
                Text(
                    "Sprawdź nowy identyfikator osobiście albo przez inny kanał: ktoś obcy też może " +
                        "podesłać swój, podszywając się pod Twój kontakt.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.size(8.dp))
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    label = { Text("Nowy identyfikator") },
                    minLines = 2,
                    modifier = Modifier.fillMaxWidth(),
                )
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val id = parseId(text)
                when {
                    id == null -> error = "To nie jest identyfikator Szeptu."
                    id == contact.id -> error = "Ten kontakt ma już ten klucz."
                    else -> onConfirm(id)
                }
            }) { Text("Zaktualizuj klucz") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Anuluj") } },
    )
}

/**
 * Asks for the archive's password, and on export whether to include the
 * identity key, on import whether to bring the old identifier back. On
 * export it rates the password as it is typed and can suggest a strong one.
 */
@Composable
fun ArchiveDialog(
    exporting: Boolean,
    onDismiss: () -> Unit,
    onConfirm: (password: String, identity: Boolean) -> Unit,
) {
    var password by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf("") }
    var identity by remember { mutableStateOf(true) }
    var visible by remember { mutableStateOf(false) }
    var suggested by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val strength = remember(password) { if (password.isEmpty()) null else passwordStrength(password) }
    val hide = if (visible) VisualTransformation.None else PasswordVisualTransformation()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (exporting) "Zapisz archiwum" else "Wczytaj archiwum") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(
                    if (exporting) {
                        "Archiwum jest szyfrowane tym hasłem (Argon2id, 128 MiB, XChaCha20-Poly1305). " +
                            "Bez hasła nie da się go odczytać, więc go nie zgub. Najlepsze jest długie hasło z kilku słów."
                    } else {
                        "Kontakty, grupy, rozmowy i ustawienia z archiwum dołączą do tych, które już tu są."
                    },
                    style = MaterialTheme.typography.bodyMedium,
                )
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("Hasło archiwum") },
                    singleLine = true,
                    visualTransformation = hide,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                if (exporting) {
                    OutlinedTextField(
                        value = repeat,
                        onValueChange = { repeat = it },
                        label = { Text("Powtórz hasło") },
                        singleLine = true,
                        visualTransformation = hide,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                        modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
                    )
                    strength?.let { rated ->
                        val score = rated.score.toInt()
                        LinearProgressIndicator(
                            progress = { (score + 1) / 5f },
                            color = when {
                                score >= 3 -> MaterialTheme.colorScheme.tertiary
                                score == 2 -> MaterialTheme.colorScheme.primary
                                else -> MaterialTheme.colorScheme.error
                            },
                            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                        )
                        Text(
                            "Hasło: ${STRENGTH_WORDS[score]}" + if (rated.acceptable) "" else " – za słabe na archiwum",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 4.dp)) {
                        TextButton(onClick = {
                            val phrase = suggestPassphrase()
                            password = phrase
                            repeat = phrase
                            visible = true
                            suggested = true
                        }) { Text("Wymyśl mocne hasło") }
                        TextButton(onClick = { visible = !visible }) { Text(if (visible) "Ukryj" else "Pokaż") }
                    }
                    if (suggested) {
                        Text(
                            "Zapisz to hasło na kartce albo w menedżerze haseł. Bez niego archiwum przepadnie.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 8.dp)
                        .clickable { identity = !identity },
                ) {
                    Checkbox(checked = identity, onCheckedChange = null)
                    Spacer(Modifier.width(8.dp))
                    Text(
                        if (exporting) {
                            "Dołącz mój klucz tożsamości: po wczytaniu zostanie ten sam identyfikator"
                        } else {
                            "Przywróć identyfikator z archiwum (obecny zostanie zastąpiony)"
                        },
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                if (exporting && identity) {
                    Text(
                        "Z kluczem tożsamości ktoś, kto zdobędzie plik i hasło, może podszywać się pod Ciebie. " +
                            "Trzymaj archiwum w bezpiecznym miejscu, a hasło osobno.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                when {
                    password.codePointCount(0, password.length) < MIN_ARCHIVE_PASSWORD ->
                        error = "Hasło musi mieć co najmniej $MIN_ARCHIVE_PASSWORD znaków."
                    exporting && strength?.acceptable != true ->
                        error = "To hasło łatwo odgadnąć. Dodaj kilka słów albo użyj przycisku Wymyśl mocne hasło."
                    exporting && password != repeat -> error = "Hasła się różnią."
                    else -> onConfirm(password, identity)
                }
            }) { Text("Wybierz plik") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Anuluj") } },
    )
}

private val STRENGTH_WORDS = listOf("bardzo słabe", "słabe", "średnie", "dobre", "bardzo dobre")

/** Matches the core's minimum, see `min_password_len`. */
private const val MIN_ARCHIVE_PASSWORD = 12
