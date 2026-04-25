import SwiftUI

struct SearchBarView: View {
    @Binding var text: String
    @FocusState.Binding var isFocused: Bool

    @State private var breathe     = false
    @State private var iconPulse   = false

    var body: some View {
        ZStack {
            glowLayers
            pill
        }
        .onAppear {
            breathe   = true
            iconPulse = true
        }
    }

    // MARK: - Ambient glow behind pill

    private var glowLayers: some View {
        ZStack {
            // Primary — tighter, brighter on focus
            Ellipse()
                .fill(Color.recallPrimary.opacity(isFocused ? 0.28 : 0.13))
                .frame(width: 300, height: 72)
                .blur(radius: isFocused ? 22 : 34)
                .scaleEffect(x: breathe ? 1.10 : 0.94, y: breathe ? 1.12 : 0.88)
                .animation(RecallAnimation.breathe(5.8), value: breathe)
                .animation(RecallAnimation.focus, value: isFocused)

            // Secondary — wide, slow
            Ellipse()
                .fill(Color.recallPrimary.opacity(isFocused ? 0.11 : 0.04))
                .frame(width: 380, height: 54)
                .blur(radius: 50)
                .scaleEffect(x: breathe ? 0.93 : 1.07, y: breathe ? 1.06 : 0.92)
                .animation(RecallAnimation.breathe(8.2), value: breathe)
                .animation(RecallAnimation.focus, value: isFocused)
        }
    }

    // MARK: - Pill

    private var pill: some View {
        HStack(spacing: 14) {
            Image(systemName: "sparkles")
                .font(.system(size: 16, weight: .light))
                .foregroundStyle(
                    isFocused
                        ? AnyShapeStyle(Color.recallPrimary)
                        : AnyShapeStyle(Color.recallPrimary.opacity(iconPulse ? 1.0 : 0.65))
                )
                .scaleEffect((!isFocused && iconPulse) ? 1.05 : 1.0)
                .animation(RecallAnimation.breathe(3.5), value: iconPulse)
                .animation(RecallAnimation.focus, value: isFocused)

            TextField("", text: $text)
                .placeholder(when: text.isEmpty) {
                    Text("Search your memory…")
                        .foregroundColor(
                            Color.recallOnSurfaceVariant
                                .opacity(isFocused ? 0.60 : 0.45)
                        )
                        .font(.system(size: 16, weight: .light))
                        .tracking(-0.2)
                }
                .focused($isFocused)
                .foregroundColor(.recallOnSurface)
                .font(.system(size: 16, weight: .light))
                .tracking(-0.2)
                .tint(.recallPrimary)

            if !text.isEmpty {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) { text = "" }
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 14))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.55))
                }
                .buttonStyle(ScaleButtonStyle())
                .transition(.scale(scale: 0.8).combined(with: .opacity))
            }
        }
        .padding(.horizontal, 22)
        .padding(.vertical, 17)
        .background(pillBackground)
        // Scale on focus: 1.02 as requested
        .scaleEffect(isFocused ? 1.02 : 1.0)
        .animation(RecallAnimation.focus, value: isFocused)
    }

    // MARK: - Pill background

    private var pillBackground: some View {
        ZStack {
            // 1. Real frosted glass — blurs whatever is behind it
            Capsule().fill(.ultraThinMaterial)

            // 2. Luminous tint — brighter than before (#2a2a2a → ~#333)
            //    isFocused: keep same brightness, just shift blue tint
            Capsule()
                .fill(
                    isFocused
                        ? Color(hex: "#1e2535").opacity(0.72)   // cool blue-dark on focus
                        : Color(hex: "#2e2e2e").opacity(0.68)   // neutral-light at rest
                )

            // 3. Top-edge inner highlight — the "light hitting the rim" effect
            Capsule()
                .fill(
                    LinearGradient(
                        colors: [
                            Color.white.opacity(isFocused ? 0.22 : 0.18),
                            Color.clear
                        ],
                        startPoint: .top,
                        endPoint: UnitPoint(x: 0.5, y: 0.38)
                    )
                )

            // 4. Bottom subtle vignette to add roundness
            Capsule()
                .fill(
                    LinearGradient(
                        colors: [Color.clear, Color.black.opacity(0.10)],
                        startPoint: UnitPoint(x: 0.5, y: 0.6),
                        endPoint: .bottom
                    )
                )

            // 5. Blue rim stroke — intensifies on focus
            Capsule()
                .strokeBorder(
                    LinearGradient(
                        colors: [
                            Color.recallPrimary.opacity(isFocused ? 0.40 : 0.14),
                            Color.recallPrimary.opacity(isFocused ? 0.14 : 0.04)
                        ],
                        startPoint: .top,
                        endPoint: .bottom
                    ),
                    lineWidth: 0.7
                )
        }
        .shadow(
            color: Color.recallPrimary.opacity(isFocused ? 0.32 : 0.14),
            radius: isFocused ? 30 : 18, x: 0, y: 0
        )
        .shadow(color: Color.black.opacity(0.50), radius: 12, x: 0, y: 6)
        .animation(RecallAnimation.focus, value: isFocused)
    }
}
