import SwiftUI

/// Message composer: attachment chips and a send/queue mode switch.
struct ComposerView: View {
    var onSend: (String) -> Void

    enum SendMode: String, CaseIterable, Identifiable {
        case send
        case queue
        var id: String { rawValue }
    }

    struct Attachment: Identifiable, Hashable {
        let id = UUID()
        let name: String
    }

    @State private var draft: String = ""
    @State private var sendMode: SendMode = .send
    @State private var attachments: [Attachment] = []
    @FocusState private var isComposerFocused: Bool

    var body: some View {
        VStack(spacing: 8) {
            if !attachments.isEmpty {
                attachmentChips
            }
            HStack(alignment: .bottom, spacing: 8) {
                attachButton
                TextField("Message", text: $draft, axis: .vertical)
                    .focused($isComposerFocused)
                    .lineLimit(1...6)
                    .padding(8)
                    .background(Color(.secondarySystemBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                submitButton
            }
            Picker("Mode", selection: $sendMode) {
                ForEach(SendMode.allCases) { mode in
                    Text(mode.rawValue.capitalized).tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        }
        .padding(8)
        .background(.bar)
    }

    private var attachmentChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(attachments) { attachment in
                    HStack(spacing: 4) {
                        Image(systemName: "paperclip")
                        Text(attachment.name).lineLimit(1)
                        Button {
                            attachments.removeAll { $0 == attachment }
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                        }
                        .buttonStyle(.borderless)
                    }
                    .font(.footnote)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color(.tertiarySystemBackground))
                    .clipShape(Capsule())
                }
            }
        }
    }

    private var attachButton: some View {
        // TODO(bindings): route to a0-platform::pickers (PhotosUI/UIDocumentPicker).
        Button {
            attachments.append(Attachment(name: "attachment-\(attachments.count + 1)"))
        } label: {
            Image(systemName: "plus.circle.fill")
                .font(.title2)
        }
        .buttonStyle(.borderless)
    }

    private var submitButton: some View {
        Button {
            let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else { return }
            // TODO(bindings): in queue mode hand off to the Rust offline queue.
            onSend(text)
            draft = ""
            attachments = []
        } label: {
            Image(systemName: sendMode == .send ? "arrow.up.circle.fill" : "tray.and.arrow.down.fill")
                .font(.title2)
        }
        .buttonStyle(.borderless)
        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    }
}
