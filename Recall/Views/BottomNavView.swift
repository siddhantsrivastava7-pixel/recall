import SwiftUI

enum RecallTab: Int, CaseIterable {
    case home, library, capture, settings

    var icon: String {
        switch self {
        case .home:     return "sparkles"
        case .library:  return "square.stack"
        case .capture:  return "plus.circle.fill"
        case .settings: return "person.crop.circle"
        }
    }

    var label: String {
        switch self {
        case .home:     return "Home"
        case .library:  return "Library"
        case .capture:  return "Capture"
        case .settings: return "Settings"
        }
    }
}

struct BottomNavView: View {
    @Binding var selectedTab: RecallTab
    @State private var appeared = false

    var body: some View {
        HStack(spacing: 0) {
            ForEach(RecallTab.allCases, id: \.rawValue) { tab in
                Spacer()
                NavButton(tab: tab, isSelected: selectedTab == tab) {
                    withAnimation(RecallAnimation.micro) {
                        selectedTab = tab
                    }
                }
                Spacer()
            }
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
        .background(floatingBackground)
        .padding(.horizontal, 44)
        .opacity(appeared ? 1 : 0)
        .offset(y: appeared ? 0 : 18)
        .onAppear {
            withAnimation(RecallAnimation.panel.delay(0.28)) {
                appeared = true
            }
        }
    }

    // MARK: - Floating background

    private var floatingBackground: some View {
        ZStack {
            // Real material blur — strongest available
            Capsule().fill(.ultraThinMaterial)

            // Dark tint over the blur to stay close to #050505 palette
            Capsule()
                .fill(Color.black.opacity(0.55))

            // Subtle inner glass gradient
            Capsule()
                .fill(
                    LinearGradient(
                        colors: [Color.white.opacity(0.10), Color.clear],
                        startPoint: .top,
                        endPoint: UnitPoint(x: 0.5, y: 0.6)
                    )
                )

            // Top-edge highlight line — brighter to simulate light hitting the rim
            Capsule()
                .fill(
                    LinearGradient(
                        colors: [Color.white.opacity(0.22), Color.clear],
                        startPoint: .top,
                        endPoint: UnitPoint(x: 0.5, y: 0.22)
                    )
                )

            // Border
            Capsule()
                .strokeBorder(Color.white.opacity(0.06), lineWidth: 0.5)
        }
        // Four shadow layers — combined for maximum floating illusion
        .shadow(color: Color.black.opacity(0.40),          radius: 40, x: 0, y: 16)
        .shadow(color: Color.black.opacity(0.75),          radius: 18, x: 0, y:  8)
        .shadow(color: Color.black.opacity(0.40),          radius:  8, x: 0, y:  3)
        .shadow(color: Color.recallPrimary.opacity(0.06),  radius: 28, x: 0, y:  0)
        .drawingGroup() // composite the entire nav into a single Metal layer
    }
}

// MARK: - Nav button

private struct NavButton: View {
    let tab: RecallTab
    let isSelected: Bool
    let action: () -> Void

    @State private var glowPulse = false

    var body: some View {
        Button(action: action) {
            ZStack {
                // Soft glow behind active icon — pulses slowly
                if isSelected {
                    Ellipse()
                        .fill(Color.recallPrimary.opacity(glowPulse ? 0.24 : 0.12))
                        .frame(width: 44, height: 28)
                        .blur(radius: glowPulse ? 14 : 18)
                        .scaleEffect(glowPulse ? 1.18 : 1.0)
                        .animation(RecallAnimation.breathe(4.0), value: glowPulse)
                }

                Image(systemName: tab.icon)
                    .font(.system(
                        size: isSelected ? 19 : 17,
                        weight: isSelected ? .regular : .light
                    ))
                    .foregroundStyle(
                        isSelected
                            ? AnyShapeStyle(Color.recallPrimary)
                            : AnyShapeStyle(Color.recallOnSurfaceVariant.opacity(0.28))
                    )
                    .scaleEffect(isSelected ? 1.08 : 1.0)
                    .animation(RecallAnimation.micro, value: isSelected)
            }
            .frame(width: 46, height: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(ScaleButtonStyle(scale: 0.88))
        .onChange(of: isSelected) { _, active in
            glowPulse = active
        }
        .onAppear {
            if isSelected { glowPulse = true }
        }
    }
}
