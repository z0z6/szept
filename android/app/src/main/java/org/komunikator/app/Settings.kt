package org.komunikator.app

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/** Profile picture, colour theme and brightness. */
@Composable
fun SettingsDialog(
    appearance: Appearance,
    dark: Boolean,
    myAvatar: ImageBitmap?,
    onPickAvatar: (android.net.Uri) -> Unit,
    onRemoveAvatar: () -> Unit,
    onDismiss: () -> Unit,
) {
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) onPickAvatar(uri)
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text("Gotowe") } },
        title = { Text("Ustawienia") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text("Twoje zdjęcie", style = MaterialTheme.typography.titleSmall)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Avatar(alias = "", online = false, picture = myAvatar, size = 64)
                    Column(modifier = Modifier.padding(start = 16.dp)) {
                        Text(
                            "Kontakty zobaczą je przy Twojej nazwie, gdy się z nimi połączysz.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = {
                                pick.launch(
                                    PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                                )
                            }) { Text("Wybierz") }
                            if (myAvatar != null) {
                                TextButton(onClick = onRemoveAvatar) { Text("Usuń") }
                            }
                        }
                    }
                }

                Text("Motyw", style = MaterialTheme.typography.titleSmall)
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

                Text("Jasność", style = MaterialTheme.typography.titleSmall)
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
                            Text(mode.label, modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
            }
        },
    )
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
                theme.name,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = if (selected) FontWeight.Bold else null,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
    }
}
