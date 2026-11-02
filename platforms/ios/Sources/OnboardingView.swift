import SwiftUI

/// First-run onboarding: enter server origin, review the scanned QR code and
/// provide credentials before the Rust core connects.
struct OnboardingView: View {
    enum Step {
        case origin
        case qrReview
        case credentials
    }

    @State private var step: Step = .origin
    @State private var origin: String = ""
    @State private var username: String = ""
    @State private var password: String = ""
    @State private var reviewedQr = false
    @AppStorage("hasCompletedOnboarding") private var hasCompletedOnboarding = false
    @AppStorage("serverOrigin") private var storedOrigin = ""

    var body: some View {
        NavigationStack {
            Form {
                switch step {
                case .origin:
                    Section("Server origin") {
                        TextField("https://agent.example.com", text: $origin)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        // TODO(bindings): Camera-based QR scan routes to
                        // a0-platform::pickers + a QR scanner; review step below.
                        Button("Scan QR code instead") { step = .qrReview }
                    }
                case .qrReview:
                    Section("Review scanned code") {
                        Text(origin.isEmpty ? "(no code scanned)" : origin)
                            .font(.caption)
                            .textSelection(.enabled)
                        Toggle("Code verified", isOn: $reviewedQr)
                    }
                case .credentials:
                    Section("Credentials") {
                        TextField("Username", text: $username)
                            .textInputAutocapitalization(.never)
                        SecureField("Password", text: $password)
                    }
                }
                Section {
                    nextButton
                }
            }
            .navigationTitle("Welcome")
        }
    }

    @ViewBuilder
    private var nextButton: some View {
        switch step {
        case .origin:
            Button("Continue") { step = .qrReview }
                .disabled(origin.isEmpty)
        case .qrReview:
            Button("Continue") { step = .credentials }
                .disabled(!reviewedQr)
        case .credentials:
            Button("Connect") {
                storedOrigin = origin
                // TODO(bindings): persist via KeychainStore + call
                // RustBridge.connect(origin, credentials) before flipping the flag.
                hasCompletedOnboarding = true
            }
            .disabled(username.isEmpty || password.isEmpty)
        }
    }
}
