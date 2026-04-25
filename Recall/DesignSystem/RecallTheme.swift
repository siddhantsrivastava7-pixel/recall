import SwiftUI

// MARK: - Color Palette (Midnight Archive Design System)

extension Color {
    static let recallBackground       = Color(hex: "#050505")
    static let recallSurface          = Color(hex: "#131313")
    static let recallSurfaceLow       = Color(hex: "#1c1b1b")
    static let recallSurfaceContainer = Color(hex: "#201f1f")
    static let recallSurfaceHigh      = Color(hex: "#2a2a2a")
    static let recallSurfaceHighest   = Color(hex: "#353534")
    static let recallSurfaceLowest    = Color(hex: "#0e0e0e")

    static let recallPrimary          = Color(hex: "#adc6ff")
    static let recallPrimaryContainer = Color(hex: "#4b8eff")
    static let recallOnPrimary        = Color(hex: "#002e69")

    static let recallOnSurface        = Color(hex: "#e5e2e1")
    static let recallOnSurfaceVariant = Color(hex: "#c1c6d7")
    static let recallOutlineVariant   = Color(hex: "#414755")
    static let recallOutline          = Color(hex: "#8b90a0")

    init(hex: String) {
        let hex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch hex.count {
        case 3:  (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6:  (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8:  (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default: (a, r, g, b) = (255, 0, 0, 0)
        }
        self.init(
            .sRGB,
            red:     Double(r) / 255,
            green:   Double(g) / 255,
            blue:    Double(b) / 255,
            opacity: Double(a) / 255
        )
    }
}

// MARK: - Typography

extension Font {
    static func recallDisplay(_ size: CGFloat = 34) -> Font {
        .system(size: size, weight: .light, design: .default)
    }
    static func recallHeadline(_ size: CGFloat = 18) -> Font {
        .system(size: size, weight: .regular, design: .default)
    }
    static func recallBody(_ size: CGFloat = 14) -> Font {
        .system(size: size, weight: .light, design: .default)
    }
    static func recallLabel(_ size: CGFloat = 9) -> Font {
        .system(size: size, weight: .medium, design: .default)
    }
    static func recallMicro(_ size: CGFloat = 8) -> Font {
        .system(size: size, weight: .semibold, design: .default)
    }
}

// MARK: - Standardised animation curves
// Default easing: cubic-bezier(0.22, 1, 0.36, 1) — calm deceleration, no overshoot

enum RecallAnimation {
    // 0.12s — physical tap feedback, scale 0.98
    static let tap    = Animation.timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.12)
    // 0.20s — micro interactions (icon state, inline toggles)
    static let micro  = Animation.timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.20)
    // 0.28s — search/focus transitions
    static let focus  = Animation.timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.28)
    // 0.35s — smooth state changes
    static let smooth = Animation.timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.35)
    // 0.40s — content appear (y offset + opacity)
    static let appear = Animation.timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.40)
    // 0.38s — panel / sheet entrance
    static let panel  = Animation.timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.38)
    // Ambient breathing — easeInOut, loops forever, no bounce
    static func breathe(_ duration: Double) -> Animation {
        .easeInOut(duration: duration).repeatForever(autoreverses: true)
    }
}

// MARK: - Spacing

enum RecallSpacing {
    static let xs:  CGFloat = 4
    static let sm:  CGFloat = 8
    static let md:  CGFloat = 16
    static let lg:  CGFloat = 24
    static let xl:  CGFloat = 32
    static let xxl: CGFloat = 48
}

// MARK: - Radius

enum RecallRadius {
    static let sm:   CGFloat = 8
    static let md:   CGFloat = 12
    static let lg:   CGFloat = 16
    static let xl:   CGFloat = 24
    static let full: CGFloat = 999
}

// MARK: - Glass Panel modifier

struct GlassPanelModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background(
                RoundedRectangle(cornerRadius: RecallRadius.lg, style: .continuous)
                    .fill(Color.recallSurfaceHigh.opacity(0.45))
                    .background(
                        .ultraThinMaterial,
                        in: RoundedRectangle(cornerRadius: RecallRadius.lg, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: RecallRadius.lg, style: .continuous)
                            .fill(
                                LinearGradient(
                                    colors: [Color.white.opacity(0.06), Color.clear],
                                    startPoint: .top,
                                    endPoint: UnitPoint(x: 0.5, y: 0.5)
                                )
                            )
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: RecallRadius.lg, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.05), lineWidth: 0.5)
                    )
            )
    }
}

extension View {
    func glassPill(isFocused: Bool = false) -> some View {
        // Legacy convenience — prefer SearchBarView's own background
        self
            .background(
                Capsule()
                    .fill(Color.recallSurfaceHigh.opacity(0.45))
                    .background(.ultraThinMaterial, in: Capsule())
                    .overlay(Capsule().strokeBorder(Color.white.opacity(0.07), lineWidth: 0.5))
                    .shadow(color: Color.recallPrimary.opacity(isFocused ? 0.18 : 0.07),
                            radius: isFocused ? 22 : 12, x: 0, y: 0)
            )
    }
    func glassPanel() -> some View {
        modifier(GlassPanelModifier())
    }
    func tapScale(_ scale: CGFloat = 0.97) -> some View {
        buttonStyle(ScaleButtonStyle(scale: scale))
    }
}

// MARK: - Button style: crisp, no-bounce tap

struct ScaleButtonStyle: ButtonStyle {
    var scale: CGFloat = 0.98
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? scale : 1)
            .animation(RecallAnimation.tap, value: configuration.isPressed)
    }
}

// MARK: - Placeholder helper

extension View {
    func placeholder<Content: View>(
        when shouldShow: Bool,
        @ViewBuilder placeholder: () -> Content
    ) -> some View {
        ZStack(alignment: .leading) {
            if shouldShow { placeholder() }
            self
        }
    }
}
