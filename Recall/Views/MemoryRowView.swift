import SwiftUI

struct MemoryRowView: View {
    let memory: Memory
    var onDelete: (() -> Void)? = nil

    @State private var isPressed  = false
    @State private var appeared   = false

    var body: some View {
        HStack(alignment: .top, spacing: 22) {
            ThumbnailView(memory: memory)
                .scaleEffect(appeared ? 1.0 : 0.96)
                .animation(RecallAnimation.appear, value: appeared)

            VStack(alignment: .leading, spacing: 9) {
                // Source type indicator + title
                HStack(spacing: 7) {
                    Image(systemName: memory.sourceIcon)
                        .font(.system(size: 10, weight: .light))
                        .foregroundColor(.recallPrimary.opacity(0.32))

                    Text(memory.title)
                        .font(.system(size: 16, weight: .regular))
                        .foregroundColor(.recallOnSurface.opacity(0.92))
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                        .tracking(-0.15)
                }

                // Preview
                if let content = memory.content ?? memory.note, !content.isEmpty {
                    Text(content)
                        .font(.system(size: 13, weight: .light))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.48))
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                        .lineSpacing(4)
                }

                // Metadata — time is primary, source is secondary
                HStack(spacing: 7) {
                    Text(memory.relativeDate.uppercased())
                        .font(.system(size: 9, weight: .medium))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.38))
                        .kerning(1.6)

                    Rectangle()
                        .fill(Color.recallOutlineVariant.opacity(0.20))
                        .frame(width: 1, height: 8)

                    Text(memory.displaySource.uppercased())
                        .font(.system(size: 9, weight: .medium))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.24))
                        .kerning(1.6)
                        .lineLimit(1)
                }
                .padding(.top, 3)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        // Subtle background tint on press — same as Apple's list highlight feel
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Color.white.opacity(isPressed ? 0.03 : 0))
        )
        .scaleEffect(isPressed ? 0.985 : 1)
        .animation(RecallAnimation.tap, value: isPressed)
        .onAppear { appeared = true }
        .contentShape(Rectangle())
        .simultaneousGesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in isPressed = true  }
                .onEnded   { _ in isPressed = false }
        )
        .contextMenu {
            if let onDelete {
                Button(role: .destructive, action: onDelete) {
                    Label("Delete", systemImage: "trash")
                }
            }
            if let urlStr = memory.url, let url = URL(string: urlStr) {
                Button {
                    UIApplication.shared.open(url)
                } label: {
                    Label("Open Link", systemImage: "safari")
                }
            }
        }
    }
}

// MARK: - Thumbnail

private struct ThumbnailView: View {
    let memory: Memory

    var body: some View {
        ZStack {
            // Base fill
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .fill(Color.recallSurfaceLow)

            // Subtle inner radial to imply depth / light source
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .fill(
                    RadialGradient(
                        colors: [
                            Color.recallPrimary.opacity(0.07),
                            Color.clear
                        ],
                        center: UnitPoint(x: 0.35, y: 0.35),
                        startRadius: 0,
                        endRadius: 55
                    )
                )

            // Icon
            Image(systemName: memory.sourceIcon)
                .font(.system(size: 20, weight: .ultraLight))
                .foregroundColor(.recallPrimary.opacity(0.32))

            // Specular border
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .strokeBorder(
                    LinearGradient(
                        colors: [Color.white.opacity(0.10), Color.white.opacity(0.03)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    ),
                    lineWidth: 0.6
                )
        }
        .frame(width: 82, height: 82)
        .shadow(color: .black.opacity(0.45), radius: 12, x: 0, y: 5)
        .fixedSize()
    }
}
