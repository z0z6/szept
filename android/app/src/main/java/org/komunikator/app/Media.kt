package org.komunikator.app

import android.content.Context
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.media.MediaPlayer
import android.media.MediaRecorder
import android.net.Uri
import android.os.Build
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.util.TypedValue
import android.view.Gravity
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.EditText
import android.widget.ImageView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.ContentInfoCompat
import androidx.core.view.ViewCompat
import androidx.core.view.inputmethod.EditorInfoCompat
import androidx.core.view.inputmethod.InputConnectionCompat
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/** What the keyboard may insert besides text: GIFs and stickers. */
private val PICTURE_TYPES = arrayOf("image/gif", "image/webp", "image/png", "image/jpeg")

/**
 * A text field that takes GIFs and stickers from the keyboard (Gboard,
 * SwiftKey, Samsung Keyboard and others), through the rich-content API that
 * keyboards use. Compose's own fields in this Material version cannot.
 */
class RichEditText(context: Context) : EditText(context) {
    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
        val connection = super.onCreateInputConnection(outAttrs) ?: return null
        val types = ViewCompat.getOnReceiveContentMimeTypes(this) ?: return connection
        // Tells the keyboard which pictures it may offer, and routes them to
        // the listener set with ViewCompat.setOnReceiveContentListener.
        EditorInfoCompat.setContentMimeTypes(outAttrs, types)
        return InputConnectionCompat.createWrapper(this, connection, outAttrs)
    }
}

/**
 * The message field. `onPicture` gets each GIF or sticker the keyboard
 * inserts; it runs at once, while the keyboard's permission to read it lasts.
 * `onCreated` hands out the view, for inserting emoji and clearing it.
 */
@Composable
fun MessageField(
    onTextChange: (String) -> Unit,
    onPicture: (Uri, String?) -> Unit,
    onCreated: (RichEditText) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MaterialTheme.colorScheme
    val textColor = colors.onSurface.toArgb()
    val hintColor = colors.onSurfaceVariant.toArgb()
    val outlineColor = colors.outline.toArgb()
    val fillColor = colors.surfaceContainerHighest.toArgb()
    AndroidView(
        modifier = modifier,
        factory = { context ->
            RichEditText(context).apply {
                hint = "Wiadomość"
                inputType = InputType.TYPE_CLASS_TEXT or
                    InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                    InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                maxLines = 4
                gravity = Gravity.CENTER_VERTICAL or Gravity.START
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
                val density = resources.displayMetrics.density
                minHeight = (48 * density).toInt()
                setPadding((14 * density).toInt(), (10 * density).toInt(), (14 * density).toInt(), (10 * density).toInt())
                addTextChangedListener(object : TextWatcher {
                    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
                    override fun afterTextChanged(s: Editable?) = onTextChange(s?.toString().orEmpty())
                })
                ViewCompat.setOnReceiveContentListener(this, PICTURE_TYPES) { _, payload ->
                    val split = payload.partition { item -> item.uri != null }
                    val pictures: ContentInfoCompat? = split.first
                    pictures?.clip?.let { clip ->
                        for (index in 0 until clip.itemCount) {
                            val uri = clip.getItemAt(index).uri ?: continue
                            onPicture(uri, clip.description.getMimeType(0))
                        }
                    }
                    // Text goes into the field as usual.
                    split.second
                }
                onCreated(this)
            }
        },
        update = { view ->
            view.setTextColor(textColor)
            view.setHintTextColor(hintColor)
            val density = view.resources.displayMetrics.density
            view.background = GradientDrawable().apply {
                cornerRadius = 24 * density
                setColor(fillColor)
                setStroke((1 * density).toInt().coerceAtLeast(1), outlineColor)
            }
        },
    )
}

/** Records one voice message at a time, as AAC in an .m4a file, which every Szept app plays. */
class VoiceRecorder(private val context: Context) {
    private var recorder: MediaRecorder? = null
    private var file: File? = null
    private var startedAt = 0L

    val recording: Boolean get() = recorder != null

    /** Milliseconds since the start. */
    fun elapsed(): Long = if (recording) System.currentTimeMillis() - startedAt else 0

    fun start(target: File) {
        cancel()
        val created = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            MediaRecorder(context)
        } else {
            @Suppress("DEPRECATION")
            MediaRecorder()
        }
        try {
            created.setAudioSource(MediaRecorder.AudioSource.MIC)
            created.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            created.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            created.setAudioChannels(1)
            created.setAudioSamplingRate(44_100)
            created.setAudioEncodingBitRate(48_000)
            created.setOutputFile(target.absolutePath)
            created.prepare()
            created.start()
        } catch (e: Exception) {
            created.release()
            target.delete()
            throw e
        }
        recorder = created
        file = target
        startedAt = System.currentTimeMillis()
    }

    /** Ends the recording. Returns the file and its length, or null when nothing usable was recorded. */
    fun finish(): Pair<File, Long>? {
        val active = recorder ?: return null
        val target = file
        val length = elapsed()
        recorder = null
        file = null
        val ok = try {
            active.stop()
            true
        } catch (e: RuntimeException) {
            // Thrown when the recording is too short to hold any sound.
            false
        } finally {
            active.release()
        }
        // Shorter than half a second is a slip of the finger.
        if (!ok || target == null || length < 500) {
            target?.delete()
            return null
        }
        return target to length
    }

    fun cancel() {
        val active = recorder ?: return
        recorder = null
        try {
            active.stop()
        } catch (e: RuntimeException) {
            // Nothing recorded yet.
        }
        active.release()
        file?.delete()
        file = null
    }
}

/** Plays one voice message at a time, for every conversation. */
object VoicePlayback {
    private var player: MediaPlayer? = null

    /** The file that is loaded, playing or paused. */
    var current by mutableStateOf<String?>(null)
        private set
    var playing by mutableStateOf(false)
        private set

    fun toggle(path: String) {
        val active = player
        if (current == path && active != null) {
            if (active.isPlaying) active.pause() else active.start()
            playing = active.isPlaying
            return
        }
        stop()
        val created = MediaPlayer()
        try {
            created.setDataSource(path)
            created.setOnCompletionListener {
                playing = false
                it.seekTo(0)
            }
            created.prepare()
            created.start()
        } catch (e: Exception) {
            created.release()
            return
        }
        player = created
        current = path
        playing = true
    }

    fun position(path: String): Int = if (current == path) player?.currentPosition ?: 0 else 0

    fun seek(path: String, ms: Int) {
        if (current == path) player?.seekTo(ms)
    }

    fun stop() {
        player?.release()
        player = null
        current = null
        playing = false
    }
}

/** 0:07, 1:42 */
fun formatDuration(ms: Long): String {
    val seconds = (ms + 500) / 1000
    return "%d:%02d".format(seconds / 60, seconds % 60)
}

/** A voice message or a GIF inside a bubble. */
@Composable
fun MediaContent(file: FileItem, onRetry: () -> Unit) {
    val path = file.path
    val ready = path != null && (file.transfer == Transfer.SAVED || file.transfer == Transfer.SHARED)
    when {
        ready && file.media == "voice" -> VoiceMessage(path!!, file.durationMs)
        ready -> GifImage(path!!)
        else -> Column(
            modifier = Modifier.widthIn(min = 180.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                if (file.media == "voice") "Wiadomość głosowa, ${formatDuration(file.durationMs)}" else "GIF",
                style = MaterialTheme.typography.bodyMedium,
            )
            if (file.transfer == Transfer.ARCHIVED) {
                Text(
                    "Z archiwum, bez zawartości.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (file.transfer == Transfer.FAILED) {
                Text(
                    "Nie udało się pobrać. Nadawca musi mieć Szept włączony.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
                Button(onClick = onRetry) { Text("Pobierz ponownie") }
            } else {
                Text(
                    "Pobieranie…",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (file.size > 0) {
                    LinearProgressIndicator(
                        progress = { file.done.toFloat() / file.size },
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }
    }
}

@Composable
private fun VoiceMessage(path: String, durationMs: Long) {
    val colors = MaterialTheme.colorScheme
    val active = VoicePlayback.current == path
    val playing = active && VoicePlayback.playing
    var position by remember(path) { mutableStateOf(0) }
    LaunchedEffect(playing, path) {
        while (playing) {
            position = VoicePlayback.position(path)
            delay(200)
        }
        position = VoicePlayback.position(path)
    }
    val total = durationMs.coerceAtLeast(1)
    Row(
        modifier = Modifier.width(240.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Surface(
            onClick = { VoicePlayback.toggle(path) },
            shape = CircleShape,
            color = colors.primary,
            modifier = Modifier
                .size(40.dp)
                .semantics { contentDescription = if (playing) "Pauza" else "Odtwórz" },
        ) {
            Canvas(modifier = Modifier.size(40.dp)) {
                val c = size.minDimension
                if (playing) {
                    val bar = Size(c * 0.1f, c * 0.36f)
                    drawRect(colors.onPrimary, Offset(c * 0.34f, c * 0.32f), bar)
                    drawRect(colors.onPrimary, Offset(c * 0.56f, c * 0.32f), bar)
                } else {
                    val triangle = Path().apply {
                        moveTo(c * 0.4f, c * 0.3f)
                        lineTo(c * 0.72f, c * 0.5f)
                        lineTo(c * 0.4f, c * 0.7f)
                        close()
                    }
                    drawPath(triangle, colors.onPrimary)
                }
            }
        }
        Slider(
            value = if (active) (position.toFloat() / total).coerceIn(0f, 1f) else 0f,
            onValueChange = { fraction ->
                if (active) {
                    position = (fraction * total).toInt()
                    VoicePlayback.seek(path, position)
                }
            },
            modifier = Modifier.weight(1f).semantics { contentDescription = "Postęp nagrania" },
            colors = SliderDefaults.colors(thumbColor = colors.primary, activeTrackColor = colors.primary),
        )
        Text(
            formatDuration(if (active && position > 0) position.toLong() else durationMs),
            style = MaterialTheme.typography.labelMedium,
            color = colors.onSurfaceVariant,
        )
    }
}

/** Shows a GIF, animated on Android 9 and newer, as a still picture before that. */
@Composable
private fun GifImage(path: String) {
    val drawable by produceState<Drawable?>(null, path) {
        value = withContext(Dispatchers.IO) { loadAnimated(path) }
    }
    val picture = drawable ?: return
    val ratio = if (picture.intrinsicHeight > 0) {
        (picture.intrinsicWidth.toFloat() / picture.intrinsicHeight).coerceIn(0.4f, 2.5f)
    } else {
        1f
    }
    AndroidView(
        factory = { context ->
            ImageView(context).apply {
                scaleType = ImageView.ScaleType.FIT_CENTER
                contentDescription = "GIF"
            }
        },
        update = { view ->
            if (view.drawable !== picture) {
                view.setImageDrawable(picture)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P && picture is AnimatedImageDrawable) {
                    picture.start()
                }
            }
        },
        modifier = Modifier
            .width(220.dp)
            .aspectRatio(ratio)
            .clip(RoundedCornerShape(12.dp)),
    )
}

private fun loadAnimated(path: String): Drawable? = try {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        ImageDecoder.decodeDrawable(ImageDecoder.createSource(File(path))) { decoder, info, _ ->
            // Big animations are shown small anyway: decode them smaller.
            val longer = maxOf(info.size.width, info.size.height)
            if (longer > 720) {
                val ratio = 720f / longer
                decoder.setTargetSize((info.size.width * ratio).toInt(), (info.size.height * ratio).toInt())
            }
        }
    } else {
        BitmapFactory.decodeFile(path)?.let { BitmapDrawable(null, it) }
    }
} catch (e: Exception) {
    null
}
