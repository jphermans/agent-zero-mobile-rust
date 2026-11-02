package um.a0.mobile.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import um.a0.mobile.rust.RustBridge

/** Chat summary shown in the list. TODO(bindings): feed from a0-chat crate. */
data class ChatSummary(
    val id: String,
    val title: String,
    val preview: String
)

@Composable
fun ChatListScreen(modifier: Modifier = Modifier, onOpenChat: (String) -> Unit = {}) {
    // TODO(bindings): observe real chat sessions via the generated facade.
    val chats = rememberPlaceholderChats()
    LazyColumn(modifier = modifier.fillMaxSize()) {
        items(chats) { chat ->
            ListItem(
                headlineContent = { Text(chat.title) },
                supportingContent = { Text(chat.preview, maxLines = 1) },
                modifier = Modifier.clickable { onOpenChat(chat.id) }
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun rememberPlaceholderChats(): List<ChatSummary> {
    val connection by RustBridge.state.collectAsState()
    val status = when (connection) {
        um.a0.mobile.rust.ConnectionState.CONNECTED -> "Connected"
        um.a0.mobile.rust.ConnectionState.CONNECTING -> "Connecting"
        else -> "Offline"
    }
    return listOf(
        ChatSummary(id = "new", title = "New chat", preview = status + " - start a conversation")
    )
}
