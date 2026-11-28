package org.komunikator.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * Buttons side by side that move to the next line when they don't fit.
 * A plain Row squeezes the last button until its label stands letter by
 * letter, which happens with longer translations and larger text.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ButtonRow(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    // FlowRowScope stays inside: in the signature it would make every caller
    // opt in to the experimental layout API too.
    FlowRow(
        modifier = modifier,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        content()
    }
}
