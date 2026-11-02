package um.a0.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * Message bubble with markdown-rendered text (TODO(bindings): swap for the
 * shared a0-ui markdown renderer once the Rust A2UI catalog is bridged) and
 * a collapsible body for long messages.
 */
data class ChatMessage(
    val id: String,
    val text: String,
    val isUser: Boolean
)

private const val COLLAPSE_THRESHOLD = 600

@Composable
fun MessageBubble(message: ChatMessage, modifier: Modifier = Modifier) {
    var expanded by remember { mutableStateOf(true) }
    val isLong = message.text.length > COLLAPSE_THRESHOLD
    val display = if (isLong && !expanded) {
        message.text.take(COLLAPSE_THRESHOLD) + "\u2026"
    } else {
        message.text
    }

    Row(modifier = modifier.fillMaxWidth()) {
        if (message.isUser) Box(Modifier.fillMaxWidth(0.35f))
        Column(
            horizontalAlignment = Alignment.Start,
            modifier = Modifier
                .weight(1f, fill = false)
                .background(
                    color = if (message.isUser) {
                        MaterialTheme.colorScheme.primaryContainer
                    } else {
                        MaterialTheme.colorScheme.surfaceVariant
                    },
                    shape = RoundedCornerShape(16.dp)
                )
                .padding(12.dp)
        ) {
            SelectionContainer {
                // TODO(bindings): real markdown via a0-ui renderer.
                Text(text = display, style = MaterialTheme.typography.bodyMedium)
            }
            if (isLong) {
                TextButton(onClick = { expanded = !expanded }) {
                    Text(if (expanded) "Show less" else "Show more")
                }
            }
        }
        if (!message.isUser) Box(Modifier.fillMaxWidth(0.35f))
    }
}
