import SwiftUI

/// Compact connection status pill shown in chat toolbars.
struct ConnectionIndicator: View {
    let state: AppModel.ConnectionState

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: iconName)
                .foregroundStyle(tint)
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Capsule().fill(tint.opacity(0.12)))
        // TODO(bindings): drive from Rust observeState stream + retry state.
    }

    private var iconName: String {
        switch state {
        case .connected: "dot.radiowaves.left.and.right"
        case .connecting: "hourglass"
        case .disconnected: "circle.slash"
        case .error: "exclamationmark.triangle"
        }
    }

    private var tint: Color {
        switch state {
        case .connected: .green
        case .connecting: .orange
        case .disconnected: .secondary
        case .error: .red
        }
    }

    private var label: String {
        switch state {
        case .connected: "Online"
        case .connecting: "Connecting"
        case .disconnected: "Offline"
        case .error: "Error"
        }
    }
}
