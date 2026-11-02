package um.a0.mobile.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import um.a0.mobile.rust.ConnectionState
import um.a0.mobile.rust.RustBridge

/** Settings screen: server connection, security and about information. */
@Composable
fun SettingsScreen(modifier: Modifier = Modifier) {
    val connection by RustBridge.state.collectAsState()
    val status = when (connection) {
        ConnectionState.CONNECTED -> "Connected"
        ConnectionState.CONNECTING -> "Connecting"
        ConnectionState.DISCONNECTED -> "Disconnected"
        ConnectionState.ERROR -> "Error"
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
    ) {
        SectionTitle("Server")
        ListItem(
            headlineContent = { Text("Connection") },
            supportingContent = { Text(status) }
        )
        // TODO(bindings): live connection test + origin edit via Rust facade.
        HorizontalDivider()
        SectionTitle("Security")
        ListItem(
            headlineContent = { Text("Credentials") },
            supportingContent = { Text("Stored in Android Keystore (hardware-backed)") }
        )
        HorizontalDivider()
        SectionTitle("About")
        ListItem(headlineContent = { Text("App") }, supportingContent = { Text("Agent Zero") })
        ListItem(headlineContent = { Text("Version") }, supportingContent = { Text("0.1.0") })
        ListItem(headlineContent = { Text("Core") }, supportingContent = { Text("a0-platform (Rust)") })
    }
}

@Composable
private fun SectionTitle(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 8.dp)
    )
}
