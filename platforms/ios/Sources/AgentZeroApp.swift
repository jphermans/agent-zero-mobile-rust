import SwiftUI

@main
struct AgentZeroApp: App {
    @StateObject private var appModel = AppModel.shared

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(appModel)
        }
    }
}

/// App-wide observable state bridged from the Rust core.
/// TODO(bindings): replace the placeholder state with the generated UniFFI
/// observer surface once A0Platform.swift names are finalized.
final class AppModel: ObservableObject {
    static let shared = AppModel()

    @Published var connectionState: ConnectionState = .disconnected

    enum ConnectionState {
        case disconnected
        case connecting
        case connected
        case error(String)
    }
}
