import SwiftUI

/// Settings screen: server connection, security and about information.
struct SettingsView: View {
    @AppStorage("serverOrigin") private var serverOrigin = ""
    @AppStorage("hasCompletedOnboarding") private var hasCompletedOnboarding = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Server") {
                    TextField("Server origin", text: $serverOrigin)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    // TODO(bindings): live connection test via RustBridge.connect().
                }
                Section("Security") {
                    // TODO(bindings): inspect stored credentials via KeychainStore.
                    Label("Credentials stored in iCloud Keychain", systemImage: "lock.shield")
                }
                Section("About") {
                    LabeledContent("App", value: "Agent Zero")
                    LabeledContent("Version", value: "0.1.0")
                    LabeledContent("Core", value: "a0-platform (Rust)")
                }
            }
            .navigationTitle("Settings")
        }
    }
}
