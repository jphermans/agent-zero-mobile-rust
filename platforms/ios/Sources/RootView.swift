import SwiftUI

/// Adaptive root: NavigationSplitView in regular width (iPad / landscape),
/// NavigationStack in compact width (architecture.md section 1.7).
struct RootView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @EnvironmentObject private var appModel: AppModel
    @AppStorage("hasCompletedOnboarding") private var hasCompletedOnboarding = false

    var body: some View {
        if hasCompletedOnboarding {
            if horizontalSizeClass == .regular {
                NavigationSplitView {
                    ChatListView()
                } detail: {
                    ContentView()
                }
            } else {
                NavigationStack {
                    ChatListView()
                }
            }
        } else {
            OnboardingView()
        }
    }
}
