package org.komunikator.app

import android.content.Context
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color

/** The colours of one variant (light or dark) of a theme. */
data class Palette(
    /** Behind everything. */
    val mist: Long,
    /** Cards, dialogs and the contact's bubbles. */
    val surface: Long,
    val ink: Long,
    val inkSoft: Long,
    val line: Long,
    val accent: Long,
    val onAccent: Long,
    /** Our own bubbles. */
    val mine: Long,
)

/** A colour theme. The desktop app has the same ten, in crates/desktop/ui/theme.js. */
data class Theme(val id: String, val name: String, val light: Palette, val dark: Palette)

val THEMES = listOf(
    Theme(
        "szept", "Szept",
        Palette(0xFFF3F4F8, 0xFFFFFFFF, 0xFF1B2140, 0xFF5A6183, 0xFFDDE0EB, 0xFF2F3A6B, 0xFFFFFFFF, 0xFFE3E6F4),
        Palette(0xFF12152A, 0xFF1B1F38, 0xFFE7E9F4, 0xFF9AA0C2, 0xFF2C3255, 0xFF9AA6F0, 0xFF12152A, 0xFF2A3163),
    ),
    Theme(
        "las", "Las",
        Palette(0xFFF1F5F1, 0xFFFFFFFF, 0xFF16261C, 0xFF54675A, 0xFFD8E3DA, 0xFF2F6B45, 0xFFFFFFFF, 0xFFDCEBDF),
        Palette(0xFF0F1912, 0xFF17241B, 0xFFE5EFE7, 0xFF98B0A0, 0xFF27392D, 0xFF8FD3A6, 0xFF0F1912, 0xFF23402D),
    ),
    Theme(
        "morze", "Morze",
        Palette(0xFFEFF6F6, 0xFFFFFFFF, 0xFF102A2D, 0xFF4D6A6D, 0xFFD3E5E6, 0xFF0F6B73, 0xFFFFFFFF, 0xFFD6ECEE),
        Palette(0xFF0B1A1C, 0xFF122527, 0xFFE2F1F2, 0xFF8FB3B6, 0xFF21393C, 0xFF7DD3DB, 0xFF0B1A1C, 0xFF1A3F43),
    ),
    Theme(
        "wrzos", "Wrzos",
        Palette(0xFFF6F2F8, 0xFFFFFFFF, 0xFF2A1A36, 0xFF6A5878, 0xFFE6DCEC, 0xFF6B3A8C, 0xFFFFFFFF, 0xFFECE0F3),
        Palette(0xFF1A1220, 0xFF241A2C, 0xFFF0E8F5, 0xFFB5A2C2, 0xFF3A2B45, 0xFFD2A8F0, 0xFF1A1220, 0xFF3D2A4E),
    ),
    Theme(
        "bursztyn", "Bursztyn",
        Palette(0xFFFAF6EE, 0xFFFFFFFF, 0xFF2E2310, 0xFF6E5F45, 0xFFECE2CF, 0xFF8A5A00, 0xFFFFFFFF, 0xFFF5E8CC),
        Palette(0xFF1C170D, 0xFF272013, 0xFFF5EEDF, 0xFFBCAE92, 0xFF3D3320, 0xFFF2C46B, 0xFF1C170D, 0xFF4A3A17),
    ),
    Theme(
        "malina", "Malina",
        Palette(0xFFFAF1F4, 0xFFFFFFFF, 0xFF33141F, 0xFF74505E, 0xFFF0D9E1, 0xFFA3204F, 0xFFFFFFFF, 0xFFF7DDE6),
        Palette(0xFF1E0F15, 0xFF2A161E, 0xFFF7E6EC, 0xFFC49AAB, 0xFF452332, 0xFFF598B8, 0xFF1E0F15, 0xFF52223A),
    ),
    Theme(
        "grafit", "Grafit",
        Palette(0xFFF3F3F4, 0xFFFFFFFF, 0xFF1E1F22, 0xFF5C5F66, 0xFFDCDDE0, 0xFF3B3E45, 0xFFFFFFFF, 0xFFE4E5E8),
        Palette(0xFF121214, 0xFF1C1C1F, 0xFFECECEE, 0xFFA0A2A8, 0xFF2F3034, 0xFFC9CCD3, 0xFF121214, 0xFF33353A),
    ),
    Theme(
        "koral", "Koral",
        Palette(0xFFFBF3F0, 0xFFFFFFFF, 0xFF34190F, 0xFF765649, 0xFFF1DDD5, 0xFFB2431F, 0xFFFFFFFF, 0xFFF8E0D7),
        Palette(0xFF1F120D, 0xFF2B1A13, 0xFFF8E9E3, 0xFFC9A294, 0xFF47291D, 0xFFFFA384, 0xFF1F120D, 0xFF55291A),
    ),
    Theme(
        "lazur", "Lazur",
        Palette(0xFFF0F5FB, 0xFFFFFFFF, 0xFF0F2238, 0xFF4E637B, 0xFFD6E2EF, 0xFF1F5FA8, 0xFFFFFFFF, 0xFFDBE8F7),
        Palette(0xFF0C1522, 0xFF132030, 0xFFE4EEF9, 0xFF94ADC8, 0xFF233650, 0xFF8EC2FF, 0xFF0C1522, 0xFF1B3A5E),
    ),
    Theme(
        "oliwka", "Oliwka",
        Palette(0xFFF5F5EE, 0xFFFFFFFF, 0xFF25260F, 0xFF63654A, 0xFFE3E4D2, 0xFF5B6118, 0xFFFFFFFF, 0xFFE9EBD2),
        Palette(0xFF16170D, 0xFF202114, 0xFFEEEFE0, 0xFFB1B394, 0xFF363822, 0xFFCDD47A, 0xFF16170D, 0xFF3B3E1B),
    ),
)

/** Light, dark, or whatever the phone uses. `key` is shared with the desktop app. */
enum class Mode(val label: String, val key: String) {
    SYSTEM("Jak w systemie", "system"),
    LIGHT("Jasny", "light"),
    DARK("Ciemny", "dark"),
}

/** Text size on top of the phone's own setting. `key` is shared with the desktop app. */
enum class TextSize(val label: String, val key: String, val scale: Float) {
    SMALL("Mały", "small", 0.9f),
    NORMAL("Zwykły", "normal", 1f),
    LARGE("Duży", "large", 1.125f),
    XLARGE("Bardzo duży", "xlarge", 1.25f),
}

/** Builds the Material colours of a theme. Status colours are the same in every theme. */
fun colorSchemeOf(theme: Theme, dark: Boolean): ColorScheme {
    val p = if (dark) theme.dark else theme.light
    val mist = Color(p.mist)
    val surface = Color(p.surface)
    val ink = Color(p.ink)
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = Color(p.accent),
        onPrimary = Color(p.onAccent),
        primaryContainer = Color(p.mine),
        onPrimaryContainer = ink,
        inversePrimary = Color(p.accent),
        secondary = Color(p.accent),
        onSecondary = Color(p.onAccent),
        secondaryContainer = Color(p.mine),
        onSecondaryContainer = ink,
        // "Online" and "delivered".
        tertiary = Color(if (dark) 0xFF6CC3A9 else 0xFF2E7D6B),
        onTertiary = mist,
        background = mist,
        onBackground = ink,
        surface = mist,
        onSurface = ink,
        surfaceVariant = Color(p.line),
        onSurfaceVariant = Color(p.inkSoft),
        surfaceTint = Color(p.accent),
        inverseSurface = ink,
        inverseOnSurface = mist,
        error = Color(if (dark) 0xFFF2B8B5 else 0xFFB3261E),
        outline = Color(p.inkSoft),
        outlineVariant = Color(p.line),
        surfaceBright = surface,
        surfaceDim = mist,
        surfaceContainerLowest = surface,
        surfaceContainerLow = surface,
        surfaceContainer = surface,
        surfaceContainerHigh = surface,
        surfaceContainerHighest = surface,
    )
}

/**
 * The person's choice of theme and brightness, kept on the phone only.
 * Changes redraw every screen at once.
 */
class Appearance(context: Context) {
    private val prefs = context.getSharedPreferences("appearance", Context.MODE_PRIVATE)

    var theme: Theme by mutableStateOf(
        THEMES.firstOrNull { it.id == prefs.getString(KEY_THEME, null) } ?: THEMES.first(),
    )
        private set

    var mode: Mode by mutableStateOf(
        Mode.entries.firstOrNull { it.name == prefs.getString(KEY_MODE, null) } ?: Mode.SYSTEM,
    )
        private set

    var textSize: TextSize by mutableStateOf(
        TextSize.entries.firstOrNull { it.name == prefs.getString(KEY_SIZE, null) } ?: TextSize.NORMAL,
    )
        private set

    fun choose(size: TextSize) {
        this.textSize = size
        prefs.edit().putString(KEY_SIZE, size.name).apply()
    }

    /** The preferences as archives and settings files carry them, the same on every platform. */
    fun toPreferences(): Map<String, String> = mapOf(
        "theme" to theme.id,
        "mode" to mode.key,
        "text_size" to textSize.key,
    )

    /** Applies preferences from an archive or a settings file. Returns how many were known. */
    fun apply(preferences: Map<String, String>): Int {
        var applied = 0
        THEMES.firstOrNull { it.id == preferences["theme"] }?.let {
            choose(it)
            applied++
        }
        Mode.entries.firstOrNull { it.key == preferences["mode"] }?.let {
            choose(it)
            applied++
        }
        TextSize.entries.firstOrNull { it.key == preferences["text_size"] }?.let {
            choose(it)
            applied++
        }
        return applied
    }

    fun choose(theme: Theme) {
        this.theme = theme
        prefs.edit().putString(KEY_THEME, theme.id).apply()
    }

    fun choose(mode: Mode) {
        this.mode = mode
        prefs.edit().putString(KEY_MODE, mode.name).apply()
    }

    fun isDark(systemDark: Boolean): Boolean = when (mode) {
        Mode.SYSTEM -> systemDark
        Mode.LIGHT -> false
        Mode.DARK -> true
    }

    private companion object {
        const val KEY_THEME = "theme"
        const val KEY_MODE = "mode"
        const val KEY_SIZE = "text_size"
    }
}
