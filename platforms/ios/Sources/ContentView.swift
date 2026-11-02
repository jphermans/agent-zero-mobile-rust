import SwiftUI

/// Detail pane: active conversation or an empty-state placeholder.
struct ContentView: View {
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        if horizontalSizeClass == .regular {
            ChatView(chatId: nil)
        } else {
            ChatView(chatId: nil)
        }
    }

    private var horizontalSizeClass: UserInterfaceSizeClass? {
        UITraitCollection.current.horizontalSizeClass == .regular ? .regular : .compact
    }
}
