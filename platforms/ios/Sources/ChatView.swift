import SwiftUI

/// Conversation screen: message list + composer with a keyboard-safe toolbar
/// (toolbar sits above the keyboard via safeAreaInsets, architecture.md 1.7).
struct ChatView: View {
    let chatId: String?

    @State private var messages: [ChatMessage] = []
    @State private var connectionState: AppModel.ConnectionState = .disconnected
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        VStack(spacing: 0) {
            messageList
            Divider()
            ComposerView { draft in
                sendMessage(draft)
            }
        }
        .navigationTitle(chatId ?? "New chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                ConnectionIndicator(state: appModel.connectionState)
            }
            // keyboard-safe toolbar groups ride the keyboard inset.
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("Queue") { /* TODO(bindings): queue offline message */ }
            }
        }
    }

    @ViewBuilder
    private var messageList: some View {
        if messages.isEmpty {
            ContentUnavailableView("No messages", systemImage: "bubble.left")
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(messages) { message in
                            MessageBubble(message: message)
                                .id(message.id)
                        }
                    }
                    .padding()
                }
                .onChange(of: messages.count) { _, _ in
                    if let last = messages.last {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
            }
        }
    }

    private func sendMessage(_ draft: String) {
        // TODO(bindings): call the generated facade connect/send surface.
        messages.append(ChatMessage(id: UUID().uuidString, text: draft, isUser: true))
    }
}

struct ChatMessage: Identifiable, Hashable {
    let id: String
    let text: String
    let isUser: Bool
}
