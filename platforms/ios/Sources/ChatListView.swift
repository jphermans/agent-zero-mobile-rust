import SwiftUI

/// Sidebar list of conversations (regular) or pushed list (compact).
struct ChatListView: View {
    // TODO(bindings): source chats from the generated UniFFI facade
    // (a0-chat crate) once binding names are finalized.
    @State private var chats: [ChatSummary] = ChatSummary.placeholderSeed

    var body: some View {
        List(chats) { chat in
            NavigationLink(value: chat.id) {
                ChatListRow(chat: chat)
            }
        }
        .navigationTitle("Agent Zero")
        .navigationDestination(for: String.self) { chatId in
            ChatView(chatId: chatId)
        }
        .refreshable { /* TODO(bindings): reload via Rust facade */ }
    }
}

struct ChatListRow: View {
    let chat: ChatSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(chat.title).font(.headline)
            Text(chat.preview)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(.vertical, 2)
    }
}

struct ChatSummary: Identifiable, Hashable {
    let id: String
    let title: String
    let preview: String

    /// Placeholder until the Rust a0-chat crate feeds real sessions.
    static let placeholderSeed: [ChatSummary] = [
        ChatSummary(id: "new", title: "New chat", preview: "Start a conversation")
    ]
}
