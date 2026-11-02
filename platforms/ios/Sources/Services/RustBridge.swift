import Foundation

/// Thin Swift wrapper around the generated UniFFI A0Platform facade.
///
/// TODO(bindings): the generated binding surface (A0Platform.swift, produced
/// by `scripts/build-ios.sh` via uniffi-bindgen 0.31 library mode) finalizes
/// the exact protocol/type names at binding generation time. Every call below
/// is isolated behind this wrapper so only THIS file changes when the real
/// names land.
enum RustBridgeError: Error {
    case notInitialized
    case bindingUnavailable(String)
}

/// Runtime callbacks handed to the Rust core at initialization.
/// TODO(bindings): conform to the generated callback protocol
/// (uniffi callback interface) instead of the closure-based placeholder.
struct RuntimeCallbacks {
    var onLog: (String, String) -> Void = { level, message in
        #if DEBUG
        print("[a0:\(level)] \(message)")
        #endif
    }
}

/// Credentials passed to connect(); secrets never outlive this struct.
struct Credentials {
    var username: String
    var password: String
}

/// Connection state surfaced to SwiftUI via AsyncStream.
enum PlatformConnectionState {
    case disconnected
    case connecting
    case connected
    case error(String)
}

final class RustBridge {
    static let shared = RustBridge()
    private var callbacks: RuntimeCallbacks?

    private init() {}

    /// Initialize the Rust core with runtime callbacks (logging etc).
    /// TODO(bindings): call generated `A0Platform.initialize(...)`.
    func initialize(_ callbacks: RuntimeCallbacks) {
        self.callbacks = callbacks
        callbacks.onLog("bridge", "RustBridge initialized (binding call pending)")
    }

    /// Connect to the server origin with credentials from the Keychain.
    /// TODO(bindings): call generated `connect(origin:credentials:)`.
    func connect(origin: String, credentials: Credentials) async throws {
        callbacks?.onLog("bridge", "connect(\(origin)) requested (binding call pending)")
    }

    /// Observe connection state changes as an AsyncStream.
    /// TODO(bindings): bridge the generated observer/callback into this stream.
    func observeState() -> AsyncStream<PlatformConnectionState> {
        AsyncStream { continuation in
            continuation.yield(.disconnected)
            // TODO(bindings): register the Rust state observer here and yield
            // every transition; keep the continuation alive until finish().
        }
    }

    /// Store or update a secret in the platform secret store.
    /// TODO(bindings): forward to the Rust `secure_store` seam which invokes
    /// the KeychainStore callback below.
    func storeSecret(_ secret: String, account: String) async throws {
        try KeychainStore.shared.set(secret, account: account)
    }
}
