package um.a0.mobile

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import um.a0.mobile.rust.RustBridge
import um.a0.mobile.ui.AppRoot

/**
 * Single-activity Compose app (ComponentActivity + setContent).
 * The Rust core is initialized before any Composable runs so the
 * generated um.a0.platform facade is ready for the UI.
 */
class MainActivity : ComponentActivity() {
    @OptIn(ExperimentalMaterial3WindowSizeClassApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        RustBridge.initialize(this)
        setContent {
            val widthSizeClass = calculateWindowSizeClass(this).widthSizeClass
            AppRoot(widthSizeClass)
        }
        // Foreground/background lifecycle hooks for the Rust core
        // (architecture.md section 4.4).
        lifecycle.addObserver(RustBridge.lifecycleObserver())
    }
}
