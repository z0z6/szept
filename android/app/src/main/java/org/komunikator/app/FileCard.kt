package org.komunikator.app

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.webkit.MimeTypeMap
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import java.io.File
import java.text.NumberFormat
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val KIND_ICON = mapOf(
    "program" to "📦",
    "archive" to "🗜️",
    "image" to "🖼️",
    "audio" to "🎵",
    "video" to "🎬",
    "document" to "📄",
)

/** A file in a conversation: what it is, how far it got, what to do with it. */
@Composable
fun FileCard(file: FileItem, mine: Boolean, from: String, onDownload: () -> Unit) {
    val context = LocalContext.current
    val colors = MaterialTheme.colorScheme
    val scope = rememberCoroutineScope()
    var saving by remember { mutableStateOf<FileItem?>(null) }
    val saveAs = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { target ->
        val source = saving?.path
        saving = null
        if (target != null && source != null) {
            scope.launch {
                val note = withContext(Dispatchers.IO) { copyOut(context, File(source), target) }
                Toast.makeText(context, note, Toast.LENGTH_LONG).show()
            }
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .background(colors.surface),
                contentAlignment = Alignment.Center,
            ) { Text(KIND_ICON[file.kind] ?: "📎", fontSize = 22.sp) }
            Column(modifier = Modifier.padding(start = 10.dp)) {
                Text(file.name, fontWeight = FontWeight.SemiBold)
                Text(
                    describe(file),
                    style = MaterialTheme.typography.bodySmall,
                    color = colors.onSurfaceVariant,
                )
            }
        }

        if (file.kind == "program" && !mine) {
            Surface(color = colors.errorContainer, shape = RoundedCornerShape(10.dp)) {
                Column(modifier = Modifier.padding(8.dp)) {
                    Text(
                        "Program od: $from. Uruchamiaj go tylko, jeśli wiesz, co to jest.",
                        style = MaterialTheme.typography.bodySmall,
                        color = colors.onErrorContainer,
                    )
                    Text(
                        "Odcisk: " + file.hash.take(16).chunked(4).joinToString(" "),
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        color = colors.onErrorContainer,
                    )
                }
            }
        }

        if (file.transfer == Transfer.DOWNLOADING) {
            LinearProgressIndicator(
                progress = { if (file.size > 0) file.done.toFloat() / file.size else 0f },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (file.transfer == Transfer.FAILED) {
            Text(
                "Pobieranie przerwane. Nadawca musi mieć Szept włączony; pobieranie wznowi się od miejsca przerwania.",
                style = MaterialTheme.typography.bodySmall,
                color = colors.error,
            )
        }

        val path = file.path
        val local = path != null && (file.transfer == Transfer.SAVED || file.transfer == Transfer.SHARED)
        if (local && file.kind == "image") {
            ImagePreview(path!!, file.name) { open(context, File(path)) }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            when {
                file.transfer == Transfer.OFFERED -> Button(onClick = onDownload) { Text("Pobierz") }
                file.transfer == Transfer.FAILED -> Button(onClick = onDownload) { Text("Wznów") }
                local -> {
                    val label = when {
                        file.name.endsWith(".apk", ignoreCase = true) -> "Zainstaluj"
                        else -> "Otwórz"
                    }
                    OutlinedButton(onClick = { open(context, File(path!!)) }) { Text(label) }
                    if (!mine) {
                        OutlinedButton(onClick = {
                            saving = file
                            saveAs.launch(file.name)
                        }) { Text("Zapisz jako…") }
                    }
                }
            }
        }
    }
}

private val sizeFormat = NumberFormat.getNumberInstance(Locale("pl", "PL")).apply {
    maximumFractionDigits = 1
}

private fun formatSize(bytes: Long): String {
    val units = listOf("B", "kB", "MB", "GB", "TB")
    var value = bytes.toDouble()
    var unit = 0
    while (value >= 1000 && unit < units.lastIndex) {
        value /= 1000
        unit += 1
    }
    return "${sizeFormat.format(value)} ${units[unit]}"
}

private fun describe(file: FileItem): String = when (file.transfer) {
    Transfer.DOWNLOADING -> "${formatSize(file.done)} z ${formatSize(file.size)}"
    Transfer.SAVED -> "${formatSize(file.size)}, zapisany"
    Transfer.SHARED -> "${formatSize(file.size)}, udostępniony z tego telefonu"
    else -> formatSize(file.size)
}

@Composable
private fun ImagePreview(path: String, name: String, onClick: () -> Unit) {
    val bitmap by produceState<ImageBitmap?>(null, path) {
        value = withContext(Dispatchers.IO) { decodeSampled(path, 720) }
    }
    bitmap?.let {
        Image(
            bitmap = it,
            contentDescription = name,
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = 260.dp)
                .clip(RoundedCornerShape(10.dp))
                .clickable(onClick = onClick),
        )
    }
}

/** Loads a picture scaled down to about `target` pixels on the long side. */
private fun decodeSampled(path: String, target: Int): ImageBitmap? = try {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(path, bounds)
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= target) sample *= 2
    val options = BitmapFactory.Options().apply { inSampleSize = sample }
    BitmapFactory.decodeFile(path, options)?.asImageBitmap()
} catch (e: Exception) {
    null
}

/**
 * Hands the file to whichever app handles its type. For an APK that is the
 * system installer, which asks the person before installing anything.
 */
private fun open(context: Context, file: File) {
    val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
    val extension = file.extension.lowercase()
    val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) ?: "application/octet-stream"
    val intent = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, mime)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    try {
        context.startActivity(intent)
    } catch (e: ActivityNotFoundException) {
        Toast.makeText(
            context,
            "Żadna aplikacja nie otwiera tego typu plików. Użyj „Zapisz jako…”.",
            Toast.LENGTH_LONG,
        ).show()
    }
}

/** Copies a received file to a place the person picked. Returns a note for them. */
private fun copyOut(context: Context, source: File, target: Uri): String = try {
    val output = context.contentResolver.openOutputStream(target) ?: error("brak dostępu")
    output.use { sink -> source.inputStream().use { it.copyTo(sink) } }
    "Zapisano ${source.name}"
} catch (e: Exception) {
    "Nie udało się zapisać: ${e.message}"
}
