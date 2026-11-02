package um.a0.mobile.rust

import android.content.Context
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.Lifecycle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Connection state surfaced to Compose via StateFlow. */
enum class ConnectionState { DISCONNECTED, CONNECTING, CONNECTED, ERROR }

/**
 * Thin Kotlin wrapper around the generated UniFFI facade (package
 * um.a0.platform, produced by scripts/build-android.sh with uniffi-bindgen
 * 0.31 in library mode against liba0_platform.so).
 *
 * TODO(bindings): the generated facade type names are finalized at binding
 * generation time; every generated call is isolated behind this wrapper so
 * only THIS file changes when the real names land.
 */
object RustBridge {

    private val _state = MutableStateFlow(ConnectionState.DISCONNECTED)
    /** Observe connection state from Compose. */
    val state: StateFlow<ConnectionState> = _state.asStateFlow()

    @Volatile
    private var initialized = false

    /** Load liba0_platform.so and initialize the Rust core with callbacks. */
    fun initialize(@Suppress("UNUSED_PARAMETER") context: Context) {
        if (initialized) return
        System.loadLibrary("a0_platform")
        // TODO(bindings): construct the generated RuntimeCallbacks object
        // implementing the UniFFI callback interface (logging, secret store
        // via KeystoreStore, etc.) and call the generated initialize().
        initialized = true
    }

    /** Connect to the server origin with credentials from KeystoreStore. */
    fun connect(origin: String, username: String, password: String) {
        _state.value = ConnectionState.CONNECTING
        // TODO(bindings): call the generated connect(origin, credentials);
        // map its async result/stream onto _state transitions.
    }

    /** Foreground/background lifecycle hooks (architecture.md section 4.4). */
    fun lifecycleObserver(): LifecycleEventObserver =
        LifecycleEventObserver { source, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> { /* TODO(bindings): resume realtime */ }
                Lifecycle.Event.ON_PAUSE -> { /* TODO(bindings): suspend realtime */ }
                else -> Unit
            }
        }
}
