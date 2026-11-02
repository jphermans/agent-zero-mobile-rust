import SwiftUI

/// Adaptive layout helpers translating architecture.md section 1.7 into
/// app-defined bands on top of the system size classes:
///   compact  < 600pt
///   medium   600-839pt
///   expanded >= 840pt
/// SwiftUI only exposes compact/regular, so `regular` maps to medium-or-above
/// and the exact band is derived from the container width where needed.
enum LayoutBand {
    case compact
    case medium
    case expanded

    init(width: CGFloat) {
        if width < 600 {
            self = .compact
        } else if width < 840 {
            self = .medium
        } else {
            self = .expanded
        }
    }
}

struct LayoutBandKey: EnvironmentKey {
    static let defaultValue: LayoutBand = .compact
}

extension EnvironmentValues {
    var layoutBand: LayoutBand {
        get { self[LayoutBandKey.self] }
        set { self[LayoutBandKey.self] = newValue }
    }
}

/// Measures the available width and injects the resolved band.
struct AdaptiveLayoutModifier: ViewModifier {
    func body(content: Content) -> some View {
        GeometryReader { proxy in
            content
                .environment(\.layoutBand, LayoutBand(width: proxy.size.width))
        }
    }
}

extension View {
    func adaptiveLayout() -> some View {
        modifier(AdaptiveLayoutModifier())
    }
}

/// Foldable/posture hook placeholder (architecture.md 1.7 foldables row).
/// TODO(bindings): wire UIKit size-class/posture changes through the Rust
/// a0-platform lifecycle callbacks when the window geometry changes.
enum FoldabilityHint {
    case unknown
}
