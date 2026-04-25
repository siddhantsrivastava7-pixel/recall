import SwiftUI

struct MemoryDetailView: View {
    let memory: Memory
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack {
            Color.recallBackground.ignoresSafeArea()

            // Ambient glow
            RadialGradient(
                colors: [Color.recallPrimary.opacity(0.06), .clear],
                center: .top,
                startRadius: 0,
                endRadius: 400
            )
            .ignoresSafeArea()

            ScrollView(showsIndicators: false) {
                VStack(alignment: .leading, spacing: 32) {

                    // Header
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(spacing: 8) {
                            Image(systemName: memory.sourceIcon)
                                .font(.system(size: 11, weight: .light))
                                .foregroundColor(.recallPrimary.opacity(0.4))

                            Text(memory.displaySource.uppercased())
                                .font(.recallLabel(9))
                                .foregroundColor(.recallOnSurfaceVariant.opacity(0.4))
                                .kerning(2)
                        }

                        Text(memory.title)
                            .font(.system(size: 26, weight: .light))
                            .foregroundColor(.recallOnSurface)
                            .lineSpacing(4)
                    }
                    .padding(.top, 8)

                    // Thumbnail placeholder region
                    ZStack {
                        RoundedRectangle(cornerRadius: 20, style: .continuous)
                            .fill(Color.recallSurfaceLow)
                            .frame(maxWidth: .infinity)
                            .frame(height: 200)

                        Image(systemName: memory.sourceIcon)
                            .font(.system(size: 48, weight: .ultraLight))
                            .foregroundColor(.recallPrimary.opacity(0.2))

                        RoundedRectangle(cornerRadius: 20, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.06), lineWidth: 0.5)
                            .frame(maxWidth: .infinity)
                            .frame(height: 200)
                    }

                    // Content
                    if let content = memory.content, !content.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            sectionLabel("CONTENT")
                            Text(content)
                                .font(.recallBody(15))
                                .foregroundColor(.recallOnSurfaceVariant.opacity(0.75))
                                .lineSpacing(6)
                        }
                    }

                    // Note
                    if let note = memory.note, !note.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            sectionLabel("YOUR NOTE")
                            Text(note)
                                .font(.recallBody(15))
                                .foregroundColor(.recallOnSurface.opacity(0.85))
                                .lineSpacing(6)
                        }
                    }

                    // URL
                    if let urlStr = memory.url, let url = URL(string: urlStr) {
                        Button {
                            UIApplication.shared.open(url)
                        } label: {
                            HStack(spacing: 10) {
                                Image(systemName: "safari")
                                    .font(.system(size: 14, weight: .light))
                                Text(urlStr)
                                    .font(.recallBody(13))
                                    .lineLimit(1)
                                Spacer()
                                Image(systemName: "arrow.up.right")
                                    .font(.system(size: 11, weight: .light))
                            }
                            .foregroundColor(.recallPrimary.opacity(0.7))
                            .padding(.horizontal, 16)
                            .padding(.vertical, 14)
                            .background(Color.recallPrimary.opacity(0.06))
                            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                        }
                        .buttonStyle(ScaleButtonStyle(scale: 0.97))
                    }

                    // Metadata footer
                    VStack(alignment: .leading, spacing: 4) {
                        metaRow("Saved", value: memory.createdAt.formatted(date: .long, time: .shortened))
                        metaRow("Status", value: memory.synced ? "Sent to desktop" : "Stored locally")
                    }
                    .padding(.top, 8)

                    Spacer(minLength: 120)
                }
                .padding(.horizontal, 24)
                .padding(.top, 24)
            }
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .navigationBarLeading) {
                Button { dismiss() } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "chevron.left")
                            .font(.system(size: 14, weight: .light))
                        Text("Back")
                            .font(.recallBody(15))
                    }
                    .foregroundColor(.recallPrimary.opacity(0.8))
                }
            }
        }
        .toolbarBackground(.hidden, for: .navigationBar)
        .preferredColorScheme(.dark)
    }

    private func sectionLabel(_ text: String) -> some View {
        Text(text)
            .font(.recallLabel(9))
            .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
            .kerning(2.5)
    }

    private func metaRow(_ label: String, value: String) -> some View {
        HStack(spacing: 8) {
            Text(label.uppercased())
                .font(.recallLabel(9))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
                .kerning(2)
            Text(value)
                .font(.recallBody(12))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.5))
        }
    }
}
