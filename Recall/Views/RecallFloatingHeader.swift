import SwiftUI

struct RecallFloatingHeader<Trailing: View>: View {
    let trailing: Trailing

    private var safeAreaTop: CGFloat {
        (UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first?.windows.first?.safeAreaInsets.top ?? 0)
    }

    var body: some View {
        HStack {
            Text("RECALL")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.48))
                .kerning(5)
            Spacer()
            trailing
        }
        .padding(.horizontal, 28)
        .frame(height: 64)
        .background(
            Rectangle()
                .fill(.ultraThinMaterial.opacity(0))
                .background(
                    LinearGradient(
                        colors: [Color.recallBackground, Color.recallBackground.opacity(0)],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
        )
        .ignoresSafeArea(edges: .top)
        .padding(.top, safeAreaTop)
    }
}

extension RecallFloatingHeader where Trailing == EmptyView {
    init() { self.trailing = EmptyView() }
}
