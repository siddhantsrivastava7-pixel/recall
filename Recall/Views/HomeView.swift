import SwiftUI

struct HomeView: View {
    @StateObject private var vm = HomeViewModel()
    @FocusState private var searchFocused: Bool
    @State private var selectedMemory: Memory?

    @State private var breathe        = false
    @State private var appeared       = false
    @State private var contentLoaded  = false
    @State private var emptyStatePulse = false

    var body: some View {
        NavigationStack {
            ZStack(alignment: .top) {
                layeredBackground

                ScrollView(showsIndicators: false) {
                    VStack(spacing: 0) {
                        // Space for the floating header
                        Color.clear.frame(height: 64)

                        searchHero
                            .opacity(appeared ? 1 : 0)
                            .scaleEffect(appeared ? 1 : 0.985)
                            .offset(y: appeared ? 0 : 12)
                            .animation(RecallAnimation.appear.delay(0.08), value: appeared)

                        if !vm.displayedMemories.isEmpty {
                            recentMemoriesSection
                        } else if !vm.searchQuery.isEmpty {
                            emptySearchState
                        } else {
                            emptyState
                        }

                        Spacer(minLength: 140)
                    }
                }

                // Focus dim overlay
                Color.black
                    .opacity(searchFocused ? 0.18 : 0)
                    .ignoresSafeArea()
                    .allowsHitTesting(false)
                    .animation(RecallAnimation.focus, value: searchFocused)

                // Floating header — sits on top of everything
                floatingHeader
            }
            .navigationBarHidden(true)
            .navigationDestination(item: $selectedMemory) { MemoryDetailView(memory: $0) }
            .onAppear {
                breathe = true
                vm.reloadFromDisk()
                withAnimation(RecallAnimation.appear.delay(0.08)) { appeared = true }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.24) {
                    withAnimation { contentLoaded = true }
                }
            }
            .onTapGesture {
                if searchFocused {
                    withAnimation(RecallAnimation.focus) { searchFocused = false }
                }
            }
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - Floating header

    private var floatingHeader: some View {
        HStack {
            Text("RECALL")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.48))
                .kerning(5)

            Spacer()

            Button {
                withAnimation(.spring(response: 0.32, dampingFraction: 0.85)) {
                    searchFocused = true
                }
            } label: {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 16, weight: .light))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.50))
            }
            .buttonStyle(ScaleButtonStyle())
        }
        .padding(.horizontal, 28)
        .frame(height: 64)
        .background(
            // Very subtle frosted blur so header reads over scrolling content
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
        .opacity(appeared ? 1 : 0)
        .animation(RecallAnimation.appear.delay(0.06), value: appeared)
    }

    private var safeAreaTop: CGFloat {
        (UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first?.windows.first?.safeAreaInsets.top ?? 0)
    }

    // MARK: - Layered background

    private var layeredBackground: some View {
        ZStack {
            // 1. Base near-black
            Color.recallBackground.ignoresSafeArea()

            // 2. Vertical gradient — top slightly lighter, bottom darker
            //    Prevents visual flatness; matches cinematic depth
            LinearGradient(
                colors: [
                    Color(hex: "#0d0d0d"),   // ~3% lighter at top
                    Color.recallBackground,
                    Color(hex: "#020202")    // slightly deeper at bottom
                ],
                startPoint: .top,
                endPoint: .bottom
            )
            .ignoresSafeArea()

            // 3. Primary radial glow — breathes slowly
            RadialGradient(
                colors: [
                    Color.recallPrimary.opacity(0.13),
                    Color.recallPrimary.opacity(0.05),
                    Color.clear
                ],
                center: UnitPoint(x: 0.5, y: 0.20),
                startRadius: 0,
                endRadius: 320
            )
            .ignoresSafeArea()
            .scaleEffect(breathe ? 1.16 : 1.0)
            .opacity(breathe ? 1.0 : 0.72)
            .animation(RecallAnimation.breathe(9), value: breathe)

            // 4. Secondary offset glow — slower, out of phase
            RadialGradient(
                colors: [Color.recallPrimary.opacity(0.06), Color.clear],
                center: UnitPoint(x: 0.56, y: 0.28),
                startRadius: 10,
                endRadius: 240
            )
            .ignoresSafeArea()
            .scaleEffect(breathe ? 0.88 : 1.10)
            .animation(RecallAnimation.breathe(13), value: breathe)

            // 5. Static grain — GPU-composited once
            GrainOverlay()
                .ignoresSafeArea()
                .blendMode(.overlay)
                .opacity(0.035)
                .allowsHitTesting(false)
        }
    }

    // MARK: - Hero search section

    private var searchHero: some View {
        VStack(spacing: 24) {
            SearchBarView(text: $vm.searchQuery, isFocused: $searchFocused)
                .padding(.horizontal, 22)

            if vm.searchQuery.isEmpty && !searchFocused {
                Text("YOUR MEMORY, STORED LOCALLY.")
                    .font(.system(size: 9, weight: .medium))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.36))
                    .kerning(3.4)
                    .transition(.opacity.animation(RecallAnimation.focus.delay(0.18)))
            }
        }
        .padding(.top, 12)
        .padding(.bottom, 48)
    }

    // MARK: - Recent memories

    private var recentMemoriesSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(vm.searchQuery.isEmpty ? "RECENT MEMORIES" : "RESULTS")
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.30))
                    .kerning(3.0)

                Spacer()

                if vm.searchQuery.isEmpty {
                    Button("Archive") {}
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundColor(.recallPrimary.opacity(0.36))
                        .kerning(2)
                        .buttonStyle(ScaleButtonStyle())
                }
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 34)

            LazyVStack(spacing: 46) {
                ForEach(Array(vm.displayedMemories.enumerated()), id: \.element.id) { index, memory in
                    Button { selectedMemory = memory } label: {
                        MemoryRowView(memory: memory) { vm.delete(memory) }
                    }
                    .buttonStyle(PlainButtonStyle())
                    .opacity(contentLoaded ? 1 : 0)
                    .offset(y: contentLoaded ? 0 : 14)
                    .animation(
                        RecallAnimation.appear.delay(0.24 + Double(index) * 0.05),
                        value: contentLoaded
                    )
                }
            }
            .padding(.horizontal, 24)
        }
    }

    // MARK: - Empty states

    private var emptySearchState: some View {
        VStack(spacing: 13) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 26, weight: .ultraLight))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.18))
            Text("Nothing found")
                .font(.system(size: 14, weight: .light))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.26))
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 80)
    }

    private var emptyState: some View {
        VStack(spacing: 20) {
            // Glowing icon
            ZStack {
                // Soft ambient glow behind icon
                Circle()
                    .fill(Color.recallPrimary.opacity(0.10))
                    .frame(width: 72, height: 72)
                    .blur(radius: 22)

                Circle()
                    .fill(Color.recallPrimary.opacity(0.05))
                    .frame(width: 100, height: 100)
                    .blur(radius: 32)

                // Icon ring
                Circle()
                    .strokeBorder(Color.recallPrimary.opacity(0.14), lineWidth: 0.6)
                    .frame(width: 56, height: 56)

                Image(systemName: "sparkles")
                    .font(.system(size: 22, weight: .ultraLight))
                    .foregroundColor(.recallPrimary.opacity(emptyStatePulse ? 1.0 : 0.55))
                    .scaleEffect(emptyStatePulse ? 1.06 : 1.0)
                    .animation(RecallAnimation.breathe(4), value: emptyStatePulse)
            }

            VStack(spacing: 10) {
                Text("Your archive is empty")
                    .font(.system(size: 15, weight: .light))
                    .foregroundColor(.recallOnSurface.opacity(0.50))

                Text("Share anything from Safari, Feedly,\nor any app to start building your memory.")
                    .font(.system(size: 13, weight: .light))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.28))
                    .multilineTextAlignment(.center)
                    .lineSpacing(5)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 48)
        .padding(.top, 72)
        .opacity(contentLoaded ? 1 : 0)
        .animation(RecallAnimation.appear.delay(0.24), value: contentLoaded)
        .onAppear { emptyStatePulse = true }
    }
}

// MARK: - Grain overlay
// Deterministic pixel hash — never re-randomises between frames.
// drawingGroup() rasterises to a Metal texture once, zero per-frame cost.

struct GrainOverlay: View {
    var body: some View {
        Canvas { context, size in
            let step: CGFloat = 1.5
            var x: CGFloat = 0
            while x < size.width {
                var y: CGFloat = 0
                while y < size.height {
                    // Fast integer hash of (x, y) — no stdlib random, fully deterministic
                    var h = UInt32(bitPattern: Int32(x) &* 374761393 &+ Int32(y) &* 668265263)
                    h = (h ^ (h >> 13)) &* 1274126177
                    h = h ^ (h >> 16)
                    let v = CGFloat(h & 0xFF) / 255.0
                    // Only draw the brighter speckles to keep it sparse
                    if v > 0.55 {
                        context.fill(
                            Path(CGRect(x: x, y: y, width: step, height: step)),
                            with: .color(.white.opacity(v * 0.55))
                        )
                    }
                    y += step
                }
                x += step
            }
        }
        .drawingGroup()   // rasterise once → Metal texture; no per-frame recompute
    }
}
