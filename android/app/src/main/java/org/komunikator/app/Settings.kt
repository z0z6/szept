package org.komunikator.app

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.DropdownMenu
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import android.content.Context
import android.net.Uri
import androidx.compose.ui.platform.LocalContext
import org.json.JSONObject

/** Profile picture, colour theme and brightness. */
@Composable
fun SettingsDialog(
    appearance: Appearance,
    dark: Boolean,
    myAvatar: ImageBitmap?,
    myName: String,
    busy: Boolean,
    onPickAvatar: (android.net.Uri) -> Unit,
    onRemoveAvatar: () -> Unit,
    onSaveName: (String) -> Unit,
    onExport: (android.net.Uri, String, Boolean) -> Unit,
    onImport: (android.net.Uri, String, Boolean) -> Unit,
    onPreviewSound: (String) -> Unit,
    wallet: uniffi.komunikator_ffi.WalletStatus?,
    onWalletToggle: () -> Unit,
    onWalletRestore: () -> Unit,
    onWalletNodes: () -> Unit,
    onDismiss: () -> Unit,
) {
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) onPickAvatar(uri)
    }
    var name by remember(myName) { mutableStateOf(myName) }
    val context = LocalContext.current
    var prefsNote by remember { mutableStateOf<String?>(null) }
    val savePrefs = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) {
            prefsNote = if (writePreferences(context, uri, appearance.toPreferences())) {
                t("Ustawienia zapisane do pliku.")
            } else {
                t("Nie udało się zapisać pliku.")
            }
        }
    }
    val loadPrefs = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            val preferences = readPreferences(context, uri)
            prefsNote = when {
                preferences == null -> t("To nie jest plik ustawień Szeptu.")
                appearance.apply(preferences) > 0 -> t("Ustawienia wczytane.")
                else -> t("W pliku nie było ustawień, które ta wersja zna.")
            }
        }
    }
    // The archive dialog asks for the password first, then the file is picked.
    var archiveMode by remember { mutableStateOf<Boolean?>(null) }
    var pending by remember { mutableStateOf<Pair<String, Boolean>?>(null) }
    val saveTo = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { uri ->
        val (password, identity) = pending ?: return@rememberLauncherForActivityResult
        pending = null
        if (uri != null) onExport(uri, password, identity)
    }
    val readFrom = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val (password, identity) = pending ?: return@rememberLauncherForActivityResult
        pending = null
        if (uri != null) onImport(uri, password, identity)
    }
    archiveMode?.let { exporting ->
        ArchiveDialog(
            exporting = exporting,
            onDismiss = { archiveMode = null },
            onConfirm = { password, identity ->
                archiveMode = null
                pending = password to identity
                if (exporting) {
                    saveTo.launch("szept-archiwum-${System.currentTimeMillis() / 1000}.szept")
                } else {
                    readFrom.launch(arrayOf("*/*"))
                }
            },
        )
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text(t("Gotowe")) } },
        title = { Text(t("Ustawienia")) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(t("Twoje zdjęcie"), style = MaterialTheme.typography.titleSmall)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Avatar(alias = "", online = false, picture = myAvatar, size = 64)
                    Column(modifier = Modifier.padding(start = 16.dp)) {
                        Text(
                            t("Kontakty zobaczą je przy Twojej nazwie, gdy się z nimi połączysz."),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        ButtonRow {
                            OutlinedButton(onClick = {
                                pick.launch(
                                    PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                                )
                            }) { Text(t("Wybierz")) }
                            if (myAvatar != null) {
                                TextButton(onClick = onRemoveAvatar) { Text(t("Usuń")) }
                            }
                        }
                    }
                }

                Text(t("Twoje imię"), style = MaterialTheme.typography.titleSmall)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        value = name,
                        onValueChange = { name = it.take(64) },
                        singleLine = true,
                        placeholder = { Text(t("np. Ala z Mokotowa")) },
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = { onSaveName(name) }, enabled = name.trim() != myName) { Text(t("Zapisz")) }
                }
                Text(
                    t("Widzą je kontakty i członkowie Twoich grup."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                LanguageChoice(appearance)

                Text(t("Motyw"), style = MaterialTheme.typography.titleSmall)
                Column(
                    modifier = Modifier.selectableGroup(),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    THEMES.chunked(3).forEach { row ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            row.forEach { theme ->
                                ThemeChoice(
                                    theme = theme,
                                    dark = dark,
                                    selected = theme.id == appearance.theme.id,
                                    onClick = { appearance.choose(theme) },
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            // Keeps the last, shorter row aligned with the others.
                            repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
                        }
                    }
                }

                Text(t("Jasność"), style = MaterialTheme.typography.titleSmall)
                Column(modifier = Modifier.selectableGroup()) {
                    Mode.entries.forEach { mode ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .selectable(
                                    selected = appearance.mode == mode,
                                    onClick = { appearance.choose(mode) },
                                    role = Role.RadioButton,
                                )
                                .padding(vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = appearance.mode == mode, onClick = null)
                            Text(t(mode.label), modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }

                Text(t("Rozmiar tekstu"), style = MaterialTheme.typography.titleSmall)
                Column(modifier = Modifier.selectableGroup()) {
                    TextSize.entries.forEach { size ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .selectable(
                                    selected = appearance.textSize == size,
                                    onClick = { appearance.choose(size) },
                                    role = Role.RadioButton,
                                )
                                .padding(vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = appearance.textSize == size, onClick = null)
                            Text(t(size.label), modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }

                SoundSettings(appearance, onPreviewSound)

                Text(t("Twoje ustawienia"), style = MaterialTheme.typography.titleSmall)
                Text(
                    t("Wygląd i preferencje zapisują się same na tym telefonie. Możesz je też zapisać do pliku i wczytać po reinstalacji albo na innym urządzeniu, także na komputerze. Archiwum zawiera je automatycznie."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                ButtonRow {
                    OutlinedButton(onClick = { savePrefs.launch("szept-ustawienia.json") }) { Text(t("Zapisz do pliku")) }
                    OutlinedButton(onClick = { loadPrefs.launch(arrayOf("application/json", "text/plain", "*/*")) }) {
                        Text(t("Wczytaj"))
                    }
                }
                prefsNote?.let { Text(it, style = MaterialTheme.typography.bodySmall) }

                WalletSettings(
                    wallet = wallet,
                    onToggle = onWalletToggle,
                    onRestore = onWalletRestore,
                    onNodes = onWalletNodes,
                )

                Text(t("Archiwum"), style = MaterialTheme.typography.titleSmall)
                Text(
                    t("Zapisz rozmowy, kontakty i grupy w zaszyfrowanym pliku, żeby wczytać je po aktualizacji, ponownej instalacji albo na nowym telefonie. Jeśli masz portfel Nano, jego seed zawsze trafia do archiwum."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (busy) {
                    Text(t("Trwa praca z archiwum…"), style = MaterialTheme.typography.bodySmall)
                } else {
                    ButtonRow {
                        OutlinedButton(onClick = { archiveMode = true }) { Text(t("Zapisz")) }
                        OutlinedButton(onClick = { archiveMode = false }) { Text(t("Wczytaj")) }
                    }
                }
            }
        },
    )
}

/** The language: the phone's, or one chosen here. Each shows in its own name. */
@Composable
private fun LanguageChoice(appearance: Appearance) {
    var open by remember { mutableStateOf(false) }
    val current = LANGUAGES.firstOrNull { it.code == appearance.language }?.name ?: t("Jak w systemie")
    Text(t("Język"), style = MaterialTheme.typography.titleSmall)
    Box {
        OutlinedButton(onClick = { open = true }) { Text(current) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(t("Jak w systemie")) },
                onClick = {
                    open = false
                    appearance.chooseLanguage("system")
                },
            )
            LANGUAGES.forEach { language ->
                DropdownMenuItem(
                    text = { Text(language.name) },
                    onClick = {
                        open = false
                        appearance.chooseLanguage(language.code)
                    },
                )
            }
        }
    }
}

/** The notification sound, previews in notifications and sounds in the app. */
@Composable
private fun SoundSettings(appearance: Appearance, onPreview: (String) -> Unit) {
    Text(t("Dźwięk i powiadomienia"), style = MaterialTheme.typography.titleSmall)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .toggleable(
                value = appearance.notifyPreview,
                onValueChange = appearance::choosePreview,
                role = Role.Checkbox,
            ),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = appearance.notifyPreview, onCheckedChange = null)
        Text(t("Pokazuj w powiadomieniach początek wiadomości"), modifier = Modifier.padding(start = 8.dp))
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .toggleable(value = appearance.sounds, onValueChange = appearance::chooseSounds, role = Role.Checkbox),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = appearance.sounds, onCheckedChange = null)
        Text(t("Dźwięki w otwartej rozmowie"), modifier = Modifier.padding(start = 8.dp))
    }
    Text(
        t("Powiadomienie zawsze pokazuje, kto pisze, i jego zdjęcie, a na zablokowanym ekranie tylko „Nowa wiadomość”. Dźwięk nowej wiadomości:"),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    val choices = SOUND_CHOICES + SoundChoice("none", t("Bez dźwięku"), t("tylko powiadomienie"))
    Column(modifier = Modifier.selectableGroup()) {
        choices.forEach { choice ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .selectable(
                        selected = appearance.sound == choice.id,
                        onClick = {
                            appearance.chooseSound(choice.id)
                            if (choice.id != "none") onPreview(choice.id)
                        },
                        role = Role.RadioButton,
                    )
                    .padding(vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = appearance.sound == choice.id, onClick = null)
                Column(modifier = Modifier.weight(1f).padding(start = 8.dp)) {
                    Text(t(choice.name), fontWeight = FontWeight.SemiBold)
                    Text(
                        t(choice.about),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (choice.id != "none") {
                    TextButton(
                        onClick = { onPreview(choice.id) },
                        modifier = Modifier.semantics { contentDescription = t("Posłuchaj: {name}", "name" to t(choice.name)) },
                    ) { Text("▶") }
                }
            }
        }
    }
}

/** A theme as a tiny conversation in its own colours. */
@Composable
private fun ThemeChoice(
    theme: Theme,
    dark: Boolean,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MaterialTheme.colorScheme
    val palette = if (dark) theme.dark else theme.light
    Surface(
        modifier = modifier.selectable(selected = selected, onClick = onClick, role = Role.RadioButton),
        shape = RoundedCornerShape(12.dp),
        color = colors.surfaceContainerHighest,
        border = BorderStroke(if (selected) 2.dp else 1.dp, if (selected) colors.primary else colors.outlineVariant),
    ) {
        Column(
            modifier = Modifier.padding(vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Canvas(modifier = Modifier.size(44.dp)) {
                val c = size.minDimension
                drawCircle(Color(palette.mist))
                val corner = CornerRadius(c * 0.12f)
                drawRoundRect(Color(palette.mine), Offset(c * 0.16f, c * 0.3f), Size(c * 0.48f, c * 0.18f), corner)
                drawRoundRect(Color(palette.accent), Offset(c * 0.3f, c * 0.54f), Size(c * 0.54f, c * 0.18f), corner)
            }
            Text(
                t(theme.name),
                style = MaterialTheme.typography.labelMedium,
                fontWeight = if (selected) FontWeight.Bold else null,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
    }
}

/** The largest settings file accepted. */
private const val MAX_SETTINGS_FILE = 64 * 1024

/** Writes preferences as the desktop app does: `{"szept": "ustawienia", "version": 1, "preferences": {...}}`. */
fun writePreferences(context: Context, uri: Uri, preferences: Map<String, String>): Boolean = try {
    val json = JSONObject()
        .put("szept", "ustawienia")
        .put("version", 1)
        .put("preferences", JSONObject(preferences))
    val output = context.contentResolver.openOutputStream(uri, "wt")
    output?.use { it.write(json.toString(2).toByteArray()) }
    output != null
} catch (e: Exception) {
    false
}

/** Reads a settings file, or returns null for anything else. */
fun readPreferences(context: Context, uri: Uri): Map<String, String>? = try {
    val bytes = context.contentResolver.openInputStream(uri)?.use { input ->
        val buffer = ByteArray(MAX_SETTINGS_FILE + 1)
        var total = 0
        while (total < buffer.size) {
            val read = input.read(buffer, total, buffer.size - total)
            if (read < 0) break
            total += read
        }
        if (total > MAX_SETTINGS_FILE) null else buffer.copyOf(total)
    }
    val json = bytes?.let { JSONObject(String(it)) }
    if (json == null || json.optString("szept") != "ustawienia") {
        null
    } else {
        val preferences = json.optJSONObject("preferences") ?: JSONObject()
        preferences.keys().asSequence()
            .mapNotNull { key -> (preferences.opt(key) as? String)?.let { key to it } }
            .toMap()
    }
} catch (e: Exception) {
    null
}
