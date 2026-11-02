import SwiftUI

/// Message bubble with markdown rendering via AttributedString and a
/// collapsible body for long messages.
struct MessageBubble: View {
    let message: ChatMessage

    @State private var isExpanded = true

    private static let collapseThreshold = 600

    private var isLong: Bool { message.text.count > Self.collapseThreshold }

    private var displayedText: String {
        guard isLong, !isExpanded else { return message.text }
        return String(message.text.prefix(Self.collapseThreshold)) + "\u{2026}"
    }

    var body: some View {
        HStack {
            if message.isUser { Spacer(minLength: 40) }
            VStack(alignment: .leading, spacing: 6) {
                // TODO(bindings): swap for the shared a0-ui markdown renderer
                // once the Rust A2UI catalog is bridged; AttributedString
                // markdown covers the Phase-1 subset.
                Text(try? AttributedString(
                    markdown: displayedText,
                    options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)
                ) ?? AttributedString(displayedText))
                    .font(.body)
                    .textSelection(.enabled)
                if isLong {
                    Button(isExpanded ? "Show less" : "Show more") {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            isExpanded.toggle()
                        }
                    }
                    .font(.footnote)
                    .buttonStyle(.borderless)
                }
            }
            .padding(12)
            .background(message.isUser ? Color.accentColor.opacity(0.18) : Color(.secondarySystemBackground))
            .clipShape(RoundedRectangle(cornerRadius: 16))
            if !message.isUser { Spacer(minLength: 40) }
        }
    }
}
