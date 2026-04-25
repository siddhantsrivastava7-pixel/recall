import SwiftUI

// Screen 3: Minimal Library / Archive
// Matches minimal_library_magical_refinement design:
// editorial grid, no cards, tonal separation only

struct LibraryView: View {
    @StateObject private var vm = HomeViewModel()
    @State private var selectedMemory: Memory?
    @State private var fadeIn = false

    private let columns = [
        GridItem(.flexible(), spacing: 12),
        GridItem(.flexible(), spacing: 12)
    ]

    var body: some View {
        NavigationStack {
            ZStack(alignment: .top) {
                Color.recallBackground.ignoresSafeArea()

                if vm.displayedMemories.isEmpty {
                    emptyState
                } else {
                    ScrollView(showsIndicators: false) {
                        VStack(alignment: .leading, spacing: 0) {
                            Color.clear.frame(height: 64)

                            // Section label
                            Text("EVERYTHING YOU'VE SAVED")
                                .font(.system(size: 9, weight: .medium))
                                .foregroundColor(.recallOnSurfaceVariant.opacity(0.28))
                                .kerning(3)
                                .padding(.horizontal, 24)
                                .padding(.top, 28)
                                .padding(.bottom, 20)

                            // Grid
                            LazyVGrid(columns: columns, spacing: 12) {
                                ForEach(vm.displayedMemories) { memory in
                                    Button {
                                        selectedMemory = memory
                                    } label: {
                                        LibraryCardView(memory: memory)
                                    }
                                    .buttonStyle(ScaleButtonStyle(scale: 0.98))
                                }
                            }
                            .padding(.horizontal, 16)
                            .opacity(fadeIn ? 1 : 0)
                            .animation(RecallAnimation.appear, value: fadeIn)

                            Spacer(minLength: 140)
                        }
                    }
                }

                RecallFloatingHeader()
            }
            .navigationBarHidden(true)
            .navigationDestination(item: $selectedMemory) { memory in
                MemoryDetailView(memory: memory)
            }
            .onAppear {
                vm.reloadFromDisk()
                withAnimation { fadeIn = true }
            }
        }
        .preferredColorScheme(.dark)
    }

    private var emptyState: some View {
        VStack(spacing: 20) {
            Image(systemName: "archivebox")
                .font(.system(size: 36, weight: .ultraLight))
                .foregroundColor(.recallPrimary.opacity(0.2))

            Text("Empty Archive")
                .font(.recallHeadline(18))
                .foregroundColor(.recallOnSurface.opacity(0.4))

            Text("Memories you save will appear here.")
                .font(.recallBody(13))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
        }
    }
}

// MARK: - Library Card

private struct LibraryCardView: View {
    let memory: Memory

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            // Image / icon region
            ZStack {
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(Color.recallSurfaceLow)
                    .aspectRatio(1.3, contentMode: .fit)

                Image(systemName: memory.sourceIcon)
                    .font(.system(size: 28, weight: .ultraLight))
                    .foregroundColor(.recallPrimary.opacity(0.25))

                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(Color.white.opacity(0.06), lineWidth: 0.5)
                    .aspectRatio(1.3, contentMode: .fit)
            }

            // Title
            Text(memory.title)
                .font(.recallBody(13))
                .foregroundColor(.recallOnSurface.opacity(0.85))
                .lineLimit(2)
                .multilineTextAlignment(.leading)

            // Meta
            HStack(spacing: 6) {
                Text(memory.relativeDate.uppercased())
                    .font(.recallLabel(8))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
                    .kerning(1.5)

                Circle()
                    .fill(Color.recallOutlineVariant.opacity(0.15))
                    .frame(width: 2, height: 2)

                Text(memory.displaySource.uppercased())
                    .font(.recallLabel(8))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
                    .kerning(1.5)
                    .lineLimit(1)
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(Color.recallSurfaceLowest)
        )
    }
}
