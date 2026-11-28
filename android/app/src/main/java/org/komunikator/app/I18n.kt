package org.komunikator.app

import android.content.Context
import android.os.LocaleList
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.json.JSONObject

/**
 * Szept in the person's language. Texts in the code are Polish, wrapped in
 * [t]; the raw resources made by tools/i18n.py map them to the chosen
 * language. Both are snapshot state, so every screen redraws at once when
 * the language changes.
 */
object I18n {
    /** The language in use, a code from [LANGUAGES]. */
    var code: String by mutableStateOf("pl")
        private set

    private var strings: Map<String, String> by mutableStateOf(emptyMap())

    /** Arabic and other languages written right to left. */
    val rtl: Boolean get() = LANGUAGES.firstOrNull { it.code == code }?.rtl == true

    /** The choice ("system" or a code), else the phone's first language Szept speaks, else English. */
    fun resolve(choice: String): String {
        if (LANGUAGES.any { it.code == choice }) return choice
        val phone = LocaleList.getDefault()
        for (index in 0 until phone.size()) {
            val language = phone[index].language
            if (LANGUAGES.any { it.code == language }) return language
        }
        return "en"
    }

    fun use(context: Context, choice: String) {
        val resolved = resolve(choice)
        strings = LANGUAGE_RESOURCES[resolved]?.let { load(context, it) } ?: emptyMap()
        code = resolved
    }

    private fun load(context: Context, resource: Int): Map<String, String> = try {
        val json = context.resources.openRawResource(resource).bufferedReader().use { JSONObject(it.readText()) }
        json.keys().asSequence().associateWith { json.getString(it) }
    } catch (e: Exception) {
        emptyMap()
    }

    fun lookup(text: String): String = strings[text] ?: text
}

/** `text` in the language in use, with each `{name}` replaced by its value. */
fun t(text: String, vararg args: Pair<String, Any?>): String {
    var out = I18n.lookup(text)
    for ((name, value) in args) out = out.replace("{$name}", value.toString())
    return out
}

/** Marks a text to translate later, where it is shown. */
fun N_(text: String): String = text
