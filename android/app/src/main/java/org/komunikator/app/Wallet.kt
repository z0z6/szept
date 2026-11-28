package org.komunikator.app

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import uniffi.komunikator_ffi.WalletStatus
import uniffi.komunikator_ffi.defaultNanoNodes
import uniffi.komunikator_ffi.formatNanoAmount
import uniffi.komunikator_ffi.nanoLink
import uniffi.komunikator_ffi.normalizeNanoAddress
import uniffi.komunikator_ffi.parseNanoAmount

/** The Nano symbol. */
const val XNO_SIGN = "Ӿ"

/** The wallet in the contact list, only while it is on. */
@Composable
fun WalletCard(state: UiState, onOpen: () -> Unit, onBackup: () -> Unit) {
    val wallet = state.wallet ?: return
    if (!wallet.enabled) return
    Card(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp).clickable(onClick = onOpen),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(t("Portfel Nano"), style = MaterialTheme.typography.titleSmall)
            Text(
                state.walletBalance?.let { "$it XNO" } ?: t("Saldo niedostępne"),
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            if (!wallet.backedUp) {
                Text(
                    t("Nie masz jeszcze kopii 24 słów. Bez niej utrata telefonu oznacza utratę środków."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
                TextButton(onClick = onBackup) { Text(t("Zrób kopię teraz")) }
            }
        }
    }
}

/** What turning the wallet on means, before anything is created. */
@Composable
fun WalletIntroDialog(onDismiss: () -> Unit, onCreate: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t("Włączyć portfel Nano?")) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    t("Szept utworzy na tym telefonie portfel kryptowaluty Nano (XNO). Kontakty zobaczą Twój adres i będą mogły przesłać Ci środki prosto z rozmowy, a Ty im. Przelewy Nano nie mają opłat i docierają w kilka sekund."),
                )
                Text(
                    t("Portfel to 24 słowa, które za chwilę zobaczysz. Zapisz je na papierze: otwierają ten sam portfel w Szepcie na innym urządzeniu i w każdym portfelu Nano (Nault, Natrium…)."),
                )
                Text(
                    t("To portfel na drobne kwoty. Przelewów nie da się cofnąć, a kto pozna 24 słowa, może zabrać środki. Do obsługi sieci Nano Szept łączy się z publicznym węzłem, który widzi Twój adres IP i adres portfela (nigdy rozmów)."),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        },
        confirmButton = { Button(onClick = onCreate) { Text(t("Utwórz portfel")) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(t("Nie teraz")) } },
    )
}

/** The 24 words, then three of them to check the copy. */
@Composable
fun WordsDialog(model: Messenger, exists: Boolean, onDismiss: () -> Unit) {
    // Loaded once the seed exists: right after turning the wallet on it may
    // still be on its way.
    val words = remember(exists) { if (exists) model.walletWords() else emptyList() }
    var quiz by remember { mutableStateOf<List<UInt>>(emptyList()) }
    val answers = remember { mutableStateListOf<String>() }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t("Twoje 24 słowa")) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (words.isEmpty()) {
                    Text(t("Tworzę portfel…"))
                    LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                } else if (quiz.isEmpty()) {
                    Text(
                        t("Zapisz je na papierze, po kolei, i schowaj. Nie rób zrzutu ekranu ani zdjęcia. Kto je pozna, może zabrać środki. Szept nigdy nie poprosi Cię o nie w rozmowie."),
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    words.chunked(2).forEachIndexed { row, pair ->
                        Row {
                            pair.forEachIndexed { column, word ->
                                Text(
                                    "${row * 2 + column + 1}. $word",
                                    fontFamily = FontFamily.Monospace,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                    }
                } else {
                    Text(t("Sprawdźmy, czy masz kopię. Wpisz słowa o tych numerach:"))
                    quiz.forEachIndexed { index, position ->
                        OutlinedTextField(
                            value = answers[index],
                            onValueChange = { answers[index] = it.trim() },
                            label = { Text(t("Słowo nr {position}", "position" to position)) },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            when {
                words.isEmpty() -> {}
                quiz.isEmpty() -> Button(onClick = {
                    quiz = model.walletQuiz()
                    answers.clear()
                    answers.addAll(quiz.map { "" })
                }) { Text(t("Zapisałem, dalej")) }
                else -> Button(onClick = {
                    if (model.confirmBackup(quiz, answers.toList())) {
                        onDismiss()
                    } else {
                        error = t("Te słowa się nie zgadzają. Sprawdź kopię i spróbuj jeszcze raz.")
                    }
                }) { Text(t("Sprawdź")) }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(t("Później")) } },
    )
}

/** Address, balance and what can be done with them. */
@Composable
fun WalletDialog(
    state: UiState,
    onRefresh: () -> Unit,
    onReceive: () -> Unit,
    onSend: () -> Unit,
    onWords: () -> Unit,
    onDismiss: () -> Unit,
) {
    val address = state.wallet?.address ?: return
    val clipboard = LocalClipboardManager.current
    val link = remember(address) { runCatching { nanoLink(address, null) }.getOrDefault(address) }
    LaunchedEffect(Unit) { onRefresh() }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t("Portfel Nano")) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                QrImage(
                    text = link,
                    modifier = Modifier
                        .size(180.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .semantics { contentDescription = t("Kod QR z Twoim adresem Nano") },
                )
                SelectionContainer {
                    Text(address, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
                }
                TextButton(onClick = { clipboard.setText(AnnotatedString(address)) }) { Text(t("Kopiuj adres")) }
                Text(
                    state.walletBalance?.let { "$it XNO" } ?: t("Saldo niedostępne"),
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                val waiting = state.walletReceivable
                if (waiting != null && waiting != "0") {
                    Text(t("Czeka na odebranie: {receivable} XNO", "receivable" to waiting), style = MaterialTheme.typography.bodySmall)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = onRefresh) { Text(t("Odśwież")) }
                    OutlinedButton(onClick = onReceive) { Text(t("Odbierz")) }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = onSend) { Text(t("Wyślij na adres")) }
                    OutlinedButton(onClick = onWords) { Text(t("24 słowa")) }
                }
                Text(
                    t("Środki od kontaktów odbierają się same, gdy Szept działa. Ten sam portfel otworzysz w każdym portfelu Nano, wpisując 24 słowa."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(t("Zamknij")) } },
    )
}

/**
 * Sends XNO in two steps: the amount (and the address, when `fixedAddress`
 * is null), then a summary to confirm. Stays open with a progress bar while
 * the payment leaves.
 */
@Composable
fun SendNanoDialog(
    recipient: String?,
    fixedAddress: String?,
    paying: Boolean,
    onDismiss: () -> Unit,
    onSend: (address: String, amount: String) -> Unit,
) {
    var address by remember { mutableStateOf(fixedAddress.orEmpty()) }
    var amount by remember { mutableStateOf("") }
    var ready by remember { mutableStateOf<Pair<String, String>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var submitted by remember { mutableStateOf(false) }
    LaunchedEffect(submitted, paying) {
        if (submitted && !paying) onDismiss()
    }
    AlertDialog(
        onDismissRequest = { if (!paying) onDismiss() },
        title = { Text(if (recipient != null) t("Wyślij XNO: {recipient}", "recipient" to recipient) else t("Wyślij XNO na adres")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                val confirmed = ready
                when {
                    paying -> {
                        Text(t("Wysyłam… Dowód pracy sieci Nano może liczyć się do minuty."))
                        LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                    }
                    confirmed != null -> {
                        Text(
                            if (recipient != null) {
                                t("Wysłać {amount} XNO do {alias}?", "amount" to confirmed.second, "alias" to recipient)
                            } else {
                                t("Wysłać {amount} XNO?", "amount" to confirmed.second)
                            },
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(confirmed.first, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
                        Text(t("Przelewu nie da się cofnąć."))
                    }
                    else -> {
                        if (fixedAddress == null) {
                            OutlinedTextField(
                                value = address,
                                onValueChange = { address = it },
                                label = { Text(t("Adres Nano")) },
                                modifier = Modifier.fillMaxWidth(),
                            )
                        } else {
                            Text(
                                "Do: ${fixedAddress.take(11)}…${fixedAddress.takeLast(6)}",
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                        OutlinedTextField(
                            value = amount,
                            onValueChange = { amount = it },
                            label = { Text(t("Kwota w XNO")) },
                            placeholder = { Text("np. 0,5") },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            val confirmed = ready
            if (!paying) {
                if (confirmed == null) {
                    Button(onClick = {
                        error = null
                        try {
                            val checkedAddress = normalizeNanoAddress(address)
                            val checkedAmount = formatNanoAmount(parseNanoAmount(amount))
                            ready = checkedAddress to checkedAmount
                        } catch (e: Exception) {
                            error = friendlyWallet(e.message.orEmpty())
                        }
                    }) { Text(t("Dalej")) }
                } else {
                    Button(onClick = {
                        submitted = true
                        onSend(confirmed.first, confirmed.second)
                    }) { Text(t("Wyślij {amount} XNO", "amount" to confirmed.second)) }
                }
            }
        },
        dismissButton = { if (!paying) TextButton(onClick = onDismiss) { Text(t("Anuluj")) } },
    )
}

@Composable
fun RestoreWalletDialog(hasWallet: Boolean, onDismiss: () -> Unit, onRestore: (String) -> Unit) {
    var words by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t("Przywróć portfel")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    t("Wpisz 24 słowa portfela Nano, z Szeptu albo z innego portfela (Nault, Natrium), lub seed w postaci 64 znaków szesnastkowych."),
                    style = MaterialTheme.typography.bodySmall,
                )
                OutlinedTextField(
                    value = words,
                    onValueChange = { words = it },
                    label = { Text(t("24 słowa")) },
                    minLines = 4,
                    modifier = Modifier.fillMaxWidth(),
                )
                if (hasWallet) {
                    Text(
                        t("Portfel, który jest teraz na tym telefonie, zostanie odłożony do pliku w danych Szeptu, nie usunięty. Upewnij się, że masz jego 24 słowa."),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            Button(onClick = { onRestore(words) }, enabled = words.isNotBlank()) { Text(t("Przywróć")) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(t("Anuluj")) } },
    )
}

@Composable
fun NanoNodesDialog(current: List<String>, onDismiss: () -> Unit, onSave: (List<String>) -> Unit) {
    val defaults = remember { defaultNanoNodes() }
    var text by remember { mutableStateOf(if (current == defaults) "" else current.joinToString("\n")) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t("Węzły Nano")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    t("Adresy węzłów RPC, jeden w wierszu, w kolejności prób. Pusto oznacza publiczne węzły domyślne. Własny węzeł w sieci lokalnej może mieć adres http://."),
                    style = MaterialTheme.typography.bodySmall,
                )
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    placeholder = { Text(defaults.joinToString("\n")) },
                    minLines = 4,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            Button(onClick = {
                onSave(text.lines().map { it.trim() }.filter { it.isNotEmpty() })
            }) { Text(t("Zapisz")) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(t("Anuluj")) } },
    )
}

/** The wallet's part of the settings. */
@Composable
fun WalletSettings(
    wallet: WalletStatus?,
    onToggle: () -> Unit,
    onRestore: () -> Unit,
    onNodes: () -> Unit,
) {
    Text(t("Portfel Nano (XNO)"), style = MaterialTheme.typography.titleSmall)
    Text(
        when {
            wallet?.enabled == true -> t("Portfel jest włączony. Kontakty widzą Twój adres Nano.")
            wallet?.exists == true ->
                t("Portfel jest wyłączony. Seed i środki zostały na tym telefonie: włącz go, żeby znowu z niego korzystać.")
            else -> t("Opcjonalny portfel kryptowaluty Nano: przesyłaj kontaktom środki prosto z rozmowy.")
        },
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton(onClick = onToggle) { Text(if (wallet?.enabled == true) t("Wyłącz") else t("Włącz")) }
        OutlinedButton(onClick = onRestore) { Text(t("Przywróć")) }
        if (wallet?.exists == true) {
            OutlinedButton(onClick = onNodes) { Text(t("Węzły")) }
        }
    }
}

/** A payment inside a message bubble. */
@Composable
fun PaymentContent(payment: PaymentItem) {
    val clipboard = LocalClipboardManager.current
    val colors = MaterialTheme.colorScheme
    val (label, color) = when (payment.status) {
        "sent" -> t("Wysłano z Twojego portfela") to colors.tertiary
        "checking" -> t("Sprawdzam w sieci Nano…") to colors.onSurfaceVariant
        "pending" -> t("Wysłane, czeka na potwierdzenie sieci") to colors.onSurfaceVariant
        "confirmed" -> t("Potwierdzone w sieci Nano") to colors.tertiary
        "invalid" -> t("Sieć Nano nie zna takiego przelewu do Ciebie. Nie ufaj tej wiadomości.") to colors.error
        "no-wallet" -> t("Twój portfel jest wyłączony: włącz go, żeby sprawdzić i odebrać") to colors.onSurfaceVariant
        else -> t("Nie udało się sprawdzić w sieci Nano") to colors.onSurfaceVariant
    }
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(
            "$XNO_SIGN ${payment.amount} XNO",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = color,
            fontWeight = if (payment.status == "invalid") FontWeight.SemiBold else null,
        )
        Text(
            t("Blok {block}… (dotknij, aby skopiować)", "block" to payment.block.take(8)),
            style = MaterialTheme.typography.labelSmall,
            color = colors.onSurfaceVariant,
            modifier = Modifier.clickable { clipboard.setText(AnnotatedString(payment.block)) },
        )
    }
}

/** The core reports wallet errors in English: translate the ones people meet. */
fun friendlyWallet(message: String): String = when {
    "the Nano wallet is off" in message -> t("Portfel Nano jest wyłączony.")
    "no Nano node answered" in message ->
        t("Żaden węzeł Nano nie odpowiada. Sprawdź połączenie z internetem albo ustaw inny węzeł.")
    "not enough XNO" in message ->
        t("Za mało środków: saldo to {amount} XNO.", "amount" to message.substringAfter("the balance is ").trim())
    "the wallet is empty" in message -> t("Portfel jest pusty: nic jeszcze do niego nie wpłynęło.")
    "has no Nano wallet" in message -> t("Ten kontakt nie ma włączonego portfela Nano.")
    "payments go only to contacts" in message -> t("Płacić z rozmowy można tylko kontaktom.")
    "the wallet's own address" in message -> t("To adres Twojego własnego portfela.")
    "checksum does not match" in message -> t("W adresie jest literówka (nie zgadza się suma kontrolna).")
    "starts with nano_" in message -> t("Adres Nano zaczyna się od nano_.")
    "60 characters" in message || "cannot contain" in message || "not a Nano address" in message ->
        t("To nie jest poprawny adres Nano.")
    "enter an amount" in message -> t("Wpisz kwotę.")
    "more than zero" in message -> t("Kwota musi być większa od zera.")
    "must be a number" in message -> t("Kwota musi być liczbą, np. 0,5.")
    "decimal places" in message -> t("Kwota może mieć najwyżej 30 miejsc po przecinku.")
    "amount is too large" in message -> t("Ta kwota jest za duża.")
    "24 words or 64 hex digits" in message -> t("Kopia portfela to 24 słowa albo 64 znaki szesnastkowe.")
    "not a valid backup" in message ->
        t("Te słowa nie są poprawną kopią portfela (literówka albo słowo nie na swoim miejscu).")
    "node address starts with" in message || "plain http" in message ->
        t("Adres węzła musi zaczynać się od https:// (http:// tylko w sieci lokalnej).")
    else -> message
}
