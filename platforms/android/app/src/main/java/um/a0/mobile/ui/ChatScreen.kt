package um.a0.mobile.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Conversation screen: LazyColumn message list + composer that stays above
 * the keyboard via imePadding() (architecture.md section 1.7).
 */
@Composable
fun ChatScreen(
    chatId: String?,
    modifier: Modifier = Modifier
) {
    var draft by remember { mutableStateOf("") }
    // TODO(bindings): real message history from the a0-chat facade.
    val messages = remember { listOf<ChatMessage>() }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    Column(modifier = modifier.fillMaxSize().imePadding()) {
        LazyColumn(
            state = listState,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(messages) { msg ->
                MessageBubble(message = msg)
            }
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(8.dp),
            verticalAlignment = Alignment.Bottom
        ) {
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.weight(1f),
                placeholder = { Text("Message") },
                maxLines = 6
            )
            IconButton(
                onClick = {
                    val text = draft.trim()
                    if (text.isNotEmpty()) {
                        // TODO(bindings): send via Rust facade; queue when offline.
                        scope.launch { /* send(text) */ }
                        draft = ""
                    }
                },
                enabled = draft.isNotBlank()
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
            }
        }
    }
}
