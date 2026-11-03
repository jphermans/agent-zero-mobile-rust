package um.a0.mobile.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Chat
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarDefaults
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.PermanentNavigationDrawer
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass

/**
 * Adaptive root scaffold (architecture.md section 1.7):
 *  - compact  (<600dp): bottom NavigationBar
 *  - medium   (600-839dp): NavigationRail
 *  - expanded (>=840dp): PermanentNavigationDrawer
 */
@Composable
fun AppRoot(widthSizeClass: WindowWidthSizeClass) {
    var destination by rememberSaveable { mutableStateOf(Destination.Chats.name) }
    val current = Destination.valueOf(destination)

    when (widthSizeClass) {
        WindowWidthSizeClass.Compact -> {
            Scaffold(
                bottomBar = {
                    NavigationBar {
                        Destination.entries.forEach { d ->
                            NavigationBarItem(
                                selected = current == d,
                                onClick = { destination = d.name },
                                icon = { Icon(d.icon, contentDescription = d.label) },
                                label = { Text(d.label) }
                            )
                        }
                    }
                }
            ) { padding ->
                AppDestination(
                    current = current,
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(padding)
                )
            }
        }
        WindowWidthSizeClass.Medium -> {
            Row(Modifier.fillMaxSize()) {
                NavigationRail {
                    Destination.entries.forEach { d ->
                        NavigationRailItemAdapter(
                            selected = current == d,
                            onClick = { destination = d.name },
                            destination = d
                        )
                    }
                }
                AppDestination(current = current, modifier = Modifier.fillMaxSize())
            }
        }
        else -> {
            PermanentNavigationDrawer(
                drawerContent = {
                    Destination.entries.forEach { d ->
                        NavigationDrawerItem(
                            label = { Text(d.label) },
                            selected = current == d,
                            onClick = { destination = d.name },
                            icon = { Icon(d.icon, contentDescription = d.label) }
                        )
                    }
                }
            ) {
                AppDestination(current = current, modifier = Modifier.fillMaxSize())
            }
        }
    }
}

@Composable
private fun NavigationRailItemAdapter(
    selected: Boolean,
    onClick: () -> Unit,
    destination: Destination
) {
    // androidx.compose.material3.NavigationRailItem
    androidx.compose.material3.NavigationRailItem(
        selected = selected,
        onClick = onClick,
        icon = { Icon(destination.icon, contentDescription = destination.label) },
        label = { Text(destination.label) }
    )
}

enum class Destination(val label: String) {
    Chats("Chats"),
    Settings("Settings");

    val icon: androidx.compose.ui.graphics.vector.ImageVector
        get() = when (this) {
            Chats -> Icons.Filled.Chat
            Settings -> Icons.Filled.Settings
        }
}

@Composable
private fun AppDestination(current: Destination, modifier: Modifier = Modifier) {
    when (current) {
        Destination.Chats -> ChatListScreen(modifier)
        Destination.Settings -> SettingsScreen(modifier)
    }
}
